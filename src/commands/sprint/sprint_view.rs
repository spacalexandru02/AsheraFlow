use std::path::Path;
use std::io;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    widgets::{Block, Borders, List, ListItem, ListState},
    layout::{Layout, Constraint, Direction, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    Terminal,
};

use crate::errors::error::Error;
use crate::core::branch_metadata::BranchMetadataManager;
use crate::core::sprint::sprint::{SprintManager, Task, TaskStatus, Sprint};
use crate::commands::checkout::CheckoutCommand;

pub struct StatefulList<T> {
    pub state: ListState,
    pub items: Vec<T>,
}

impl<T> StatefulList<T> {
    fn with_items(items: Vec<T>) -> StatefulList<T> {
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(0));
        }
        StatefulList {
            state,
            items,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

struct SprintData {
    name: String,
    branch: String,
    is_active: bool,
    is_current: bool,
    expanded: bool,
    tasks: Vec<Task>,
    start_date: Option<String>,
    end_date: Option<String>,
}

pub struct SprintApp {
    sprints: StatefulList<SprintData>,
    current_branch: String,
    task_mode: bool,
    task_index: usize,
}

impl SprintApp {
    fn new(sprint_data: Vec<SprintData>, current_branch: String) -> SprintApp {
        SprintApp {
            sprints: StatefulList::with_items(sprint_data),
            current_branch,
            task_mode: false,
            task_index: 0,
        }
    }

    fn toggle_expanded(&mut self) {
        if let Some(i) = self.sprints.state.selected() {
            self.sprints.items[i].expanded = !self.sprints.items[i].expanded;
        }
    }

    fn enter_task_mode(&mut self) {
        if let Some(i) = self.sprints.state.selected() {
            if !self.sprints.items[i].tasks.is_empty() {
                self.sprints.items[i].expanded = true;
                self.task_mode = true;
                self.task_index = 0;
            }
        }
    }

    fn exit_task_mode(&mut self) {
        self.task_mode = false;
        self.task_index = 0;
    }

    fn next_item(&mut self) {
        if self.task_mode {
            if let Some(i) = self.sprints.state.selected() {
                let tasks = &self.sprints.items[i].tasks;
                if !tasks.is_empty() && self.task_index < tasks.len() - 1 {
                    self.task_index += 1;
                }
            }
        } else {
            self.sprints.next();
            self.task_index = 0;
        }
    }

    fn previous_item(&mut self) {
        if self.task_mode {
            if self.task_index > 0 {
                self.task_index -= 1;
            }
        } else {
            self.sprints.previous();
            self.task_index = 0;
        }
    }

    fn checkout_sprint(&mut self) -> Result<(), Error> {
        if let Some(i) = self.sprints.state.selected() {
            let sprint = &self.sprints.items[i];
            CheckoutCommand::execute(&sprint.branch)?;
            
            // Update current branch status
            self.current_branch = sprint.branch.clone();
            
            // Update is_current flag for all sprints
            for s in &mut self.sprints.items {
                s.is_current = s.branch == self.current_branch;
            }
        }
        Ok(())
    }

    fn checkout_task(&mut self) -> Result<(), Error> {
        if let Some(sprint_idx) = self.sprints.state.selected() {
            let sprint = &self.sprints.items[sprint_idx];
            if self.task_index < sprint.tasks.len() {
                let task = &sprint.tasks[self.task_index];
                
                // Format task branch name: sprint-sprintX-task-TASKID
                let task_branch = format!("{}-task-{}", sprint.branch, task.id);
                
                // Checkout the task branch
                CheckoutCommand::execute(&task_branch)?;
                
                // Update current branch
                self.current_branch = task_branch;
                
                // Update is_current flag
                for s in &mut self.sprints.items {
                    s.is_current = false; // Reset all
                }
            }
        }
        Ok(())
    }
}

pub struct SprintViewCommand;

impl SprintViewCommand {
    pub fn execute() -> Result<(), Error> {
        // Initialize the repository path
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository: .ash directory not found".into()));
        }
        
        // Create branch metadata manager
        let branch_manager = BranchMetadataManager::new(root_path);
        
        // Get current branch for comparison
        let current_branch = branch_manager.get_current_branch()?;
        
        // Create a sprint manager to get all sprints
        let sprint_manager = SprintManager::new(root_path);
        
        // Get all sprint branches
        let all_sprint_branches = branch_manager.get_all_sprint_branches()?;
        
        // Find active sprint
        let active_sprint = branch_manager.find_active_sprint()?;
        
        // Process sprints data
        let mut sprint_data = Vec::new();
        
        for branch in all_sprint_branches {
            let name = if branch.starts_with("sprint-") {
                branch[7..].to_string()
            } else {
                branch.clone()
            };
            
            let is_active = if let Some((active_branch, _)) = &active_sprint {
                branch == *active_branch
            } else {
                false
            };
            
            let branch_name = if branch.starts_with("sprint-") {
                branch.clone()
            } else {
                format!("sprint-{}", branch)
            };
            
            let is_current = branch_name == current_branch;
            
            // Get tasks for this sprint
            let tasks_map = sprint_manager.get_sprint_tasks(&branch_name)?;
            let tasks: Vec<Task> = tasks_map.values().cloned().collect();
            
            // Inițializăm datele de perioadă ca None
            let mut start_date_str = None;
            let mut end_date_str = None;
            
            // Obțin datele despre perioadă din BranchMetadataManager
            if let Ok(Some(meta)) = branch_manager.get_sprint_metadata(&branch_name) {
                // Formatăm data de început
                let start_formatted = chrono::NaiveDateTime::from_timestamp_opt(meta.start_timestamp as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string());
                start_date_str = start_formatted;
                
                // Calculăm și formatăm data de sfârșit
                let end_timestamp = meta.end_timestamp();
                let end_formatted = chrono::NaiveDateTime::from_timestamp_opt(end_timestamp as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string());
                end_date_str = end_formatted;
            }
            
            sprint_data.push(SprintData {
                name,
                branch: branch_name,
                is_active,
                is_current,
                expanded: false,
                tasks,
                start_date: start_date_str,
                end_date: end_date_str,
            });
        }
        
        // Setup terminal UI
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        
        // Create app state
        let app = SprintApp::new(sprint_data, current_branch);
        
        // Run the app
        let res = Self::run_app(&mut terminal, app);
        
        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        
        if let Err(err) = res {
            return Err(Error::Generic(format!("Error running terminal UI: {}", err)));
        }
        
        Ok(())
    }
    
    fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: SprintApp) -> io::Result<()> {
        loop {
            terminal.draw(|f| Self::ui(f, &mut app))?;
            
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Down => app.next_item(),
                    KeyCode::Up => app.previous_item(),
                    KeyCode::Right | KeyCode::Enter => {
                        if app.task_mode {
                            // Checkout the selected task
                            if let Err(e) = app.checkout_task() {
                                eprintln!("Error checking out task: {}", e);
                            } else {
                                return Ok(());  // Exit after successful checkout
                            }
                        } else {
                            app.enter_task_mode();
                        }
                    },
                    KeyCode::Left | KeyCode::Esc => {
                        if app.task_mode {
                            app.exit_task_mode();
                        } else {
                            app.toggle_expanded();
                        }
                    },
                    KeyCode::Char('c') => {
                        if let Err(e) = app.checkout_sprint() {
                            // Handle checkout error
                            // In a real app, we might want to show this error in the UI
                            eprintln!("Error checking out sprint: {}", e);
                        }
                    },
                    _ => {}
                }
            }
        }
    }
    
    fn ui<B: Backend>(f: &mut tui::Frame<B>, app: &mut SprintApp) {
        let size = f.size();
        
        // Create the outer block
        let block = Block::default()
            .title("Sprint Manager")
            .borders(Borders::ALL);
        f.render_widget(block, size);
        
        // Create list items
        let items: Vec<ListItem> = app.sprints.items.iter().enumerate().map(|(sprint_idx, sprint)| {
            let mut lines = Vec::new();
            
            // Format the sprint name with indicators
            let mut sprint_name = format!("{}", sprint.name);
            
            if sprint.is_current {
                sprint_name = format!("* {}", sprint_name);
            }
            
            let sprint_style = if sprint.is_active {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };

            // Determine if this sprint is the selected one
            let selected_sprint = app.sprints.state.selected() == Some(sprint_idx);
            
            // Add indicator for selected sprint
            let sprint_text = if selected_sprint && !app.task_mode {
                format!("> {}", sprint_name)
            } else {
                format!("  {}", sprint_name)
            };
            
            // Add period information if available
            let period_text = match (&sprint.start_date, &sprint.end_date) {
                (Some(start), Some(end)) => format!(" ({} - {})", start, end),
                (Some(start), None) => format!(" (from {})", start),
                (None, Some(end)) => format!(" (until {})", end),
                (None, None) => "".to_string(),
            };
            
            lines.push(Spans::from(vec![
                Span::styled(sprint_text, sprint_style),
                Span::styled(period_text, Style::default().fg(Color::Cyan)),
            ]));
            
            // If expanded, add task items
            if sprint.expanded {
                for (task_idx, task) in sprint.tasks.iter().enumerate() {
                    // Check if this task is currently selected in task mode
                    let task_selected = app.task_mode && selected_sprint && task_idx == app.task_index;
                    
                    // Format task with status
                    let (status_str, status_style) = match task.status {
                        TaskStatus::Todo => ("[TODO]", Style::default().fg(Color::Red)),
                        TaskStatus::InProgress => ("[IN PROGRESS]", Style::default().fg(Color::Yellow)),
                        TaskStatus::Done => ("[DONE]", Style::default().fg(Color::Green)),
                    };
                    
                    // Add indicator for selected task
                    let task_prefix = if task_selected {
                        "> - "
                    } else {
                        "  - "
                    };
                    
                    // Format for story points if available
                    let points_str = match task.story_points {
                        Some(points) => format!(" ({}sp)", points),
                        None => "".to_string(),
                    };
                    
                    lines.push(Spans::from(vec![
                        Span::raw(task_prefix),
                        Span::styled(status_str, status_style),
                        Span::raw(" "),
                        Span::styled(
                            format!("{} {}{}", task.id, task.description, points_str), 
                            Style::default()
                        ),
                    ]));
                }
            }
            
            ListItem::new(lines)
        }).collect();
        
        // Create the list widget
        let list = List::new(items)
            .block(Block::default().title("Sprints").borders(Borders::NONE))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        
        // Create help text
        let help_text = vec![
            Spans::from(vec![
                Span::raw("↑/↓: Navigate  "),
                if app.task_mode {
                    Span::raw("Enter: Select Task  ")
                } else {
                    Span::raw("Enter: Enter Task Mode  ")
                },
                if app.task_mode {
                    Span::raw("Esc: Exit Task Mode  ")
                } else {
                    Span::raw("Left/Right: Expand/Collapse  ")
                },
                Span::raw("C: Checkout Sprint  "),
                Span::raw("Q: Quit"),
            ]),
        ];
        
        let help_paragraph = tui::widgets::Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::NONE));
        
        // Layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
            ].as_ref())
            .split(f.size());
        
        // Render the list with state
        f.render_stateful_widget(list, chunks[0], &mut app.sprints.state);
        
        // Render help text
        f.render_widget(help_paragraph, chunks[1]);
    }
} 