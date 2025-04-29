use std::path::Path;
use std::io;
use std::collections::HashMap;
use std::time::{Duration, Instant};
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
use crate::core::sprint::sprint::{SprintManager, TaskStatus, Sprint};
use crate::core::commit_metadata::{CommitMetadataManager, TaskMetadata};
use crate::commands::checkout::CheckoutCommand;
use crate::core::repository::repository::Repository;

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
                    // Revin la începutul listei când ajung la sfârșit
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
                    // Trec la sfârșitul listei când sunt la început
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

#[derive(Clone)]
struct TaskInfo {
    id: String,
    description: String,
    story_points: Option<u32>,
    status: crate::core::commit_metadata::TaskStatus,
    sprint_branch: Option<String>,
}

pub struct TaskApp {
    tasks: StatefulList<TaskInfo>,
    current_branch: String,
    filter_mode: FilterMode,
}

#[derive(PartialEq)]
enum FilterMode {
    All,
    CurrentSprint,
    ActiveTasks,
}

impl TaskApp {
    fn new(tasks: Vec<TaskInfo>, current_branch: String) -> TaskApp {
        TaskApp {
            tasks: StatefulList::with_items(tasks),
            current_branch,
            filter_mode: FilterMode::CurrentSprint,
        }
    }

    fn next_filter_mode(&mut self) {
        self.filter_mode = match self.filter_mode {
            FilterMode::All => FilterMode::CurrentSprint,
            FilterMode::CurrentSprint => FilterMode::ActiveTasks,
            FilterMode::ActiveTasks => FilterMode::All,
        };
        
        // Resetăm selecția pentru a asigura că e vizibilă după schimbarea filtrului
        if !self.tasks.items.is_empty() {
            self.tasks.state.select(Some(0));
        }
    }
    
    // Navigare adaptată la task-urile filtrate
    fn next_filtered_task(&mut self) {
        // Obține indexul curent selectat
        let current_selection = self.tasks.state.selected();
        
        // Obține lista de indici ai task-urilor filtrate
        let filtered_indices = self.get_filtered_indices();
        
        if filtered_indices.is_empty() {
            return; // Nu avem task-uri filtrate
        }
        
        // Găsește următorul index filtrat
        if let Some(current_idx) = current_selection {
            // Găsește poziția indexului curent în lista de indici filtrați
            let position = filtered_indices.iter().position(|&idx| idx == current_idx);
            
            if let Some(pos) = position {
                // Dacă am găsit poziția, selecționăm următorul sau revenim la început
                if pos + 1 < filtered_indices.len() {
                    self.tasks.state.select(Some(filtered_indices[pos + 1]));
                } else {
                    // Revenire la primul element filtrat
                    self.tasks.state.select(Some(filtered_indices[0]));
                }
            } else {
                // Dacă indexul curent nu e în lista filtrată, selectează primul
                self.tasks.state.select(Some(filtered_indices[0]));
            }
        } else {
            // Nicio selecție curentă, selectează primul element filtrat
            self.tasks.state.select(Some(filtered_indices[0]));
        }
    }
    
    // Navigare înapoi adaptată la task-urile filtrate
    fn previous_filtered_task(&mut self) {
        // Obține indexul curent selectat
        let current_selection = self.tasks.state.selected();
        
        // Obține lista de indici ai task-urilor filtrate
        let filtered_indices = self.get_filtered_indices();
        
        if filtered_indices.is_empty() {
            return; // Nu avem task-uri filtrate
        }
        
        // Găsește indexul anterior filtrat
        if let Some(current_idx) = current_selection {
            // Găsește poziția indexului curent în lista de indici filtrați
            let position = filtered_indices.iter().position(|&idx| idx == current_idx);
            
            if let Some(pos) = position {
                // Dacă am găsit poziția, selecționăm anteriorul sau mergem la sfârșit
                if pos > 0 {
                    self.tasks.state.select(Some(filtered_indices[pos - 1]));
                } else {
                    // Salt la ultimul element filtrat
                    self.tasks.state.select(Some(filtered_indices[filtered_indices.len() - 1]));
                }
            } else {
                // Dacă indexul curent nu e în lista filtrată, selectează primul
                self.tasks.state.select(Some(filtered_indices[0]));
            }
        } else {
            // Nicio selecție curentă, selectează primul element filtrat
            self.tasks.state.select(Some(filtered_indices[0]));
        }
    }
    
    // Obține lista de indici ai task-urilor care trec de filtru
    fn get_filtered_indices(&self) -> Vec<usize> {
        self.tasks.items.iter().enumerate()
            .filter(|(_, task)| {
                match self.filter_mode {
                    FilterMode::All => true,
                    FilterMode::CurrentSprint => {
                        if let Some(branch) = &task.sprint_branch {
                            branch == &self.current_branch || 
                            (self.current_branch.contains("-task-") && branch.contains(&self.current_branch.split("-task-").next().unwrap_or("")))
                        } else {
                            false
                        }
                    },
                    FilterMode::ActiveTasks => {
                        task.status == crate::core::commit_metadata::TaskStatus::InProgress
                    }
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn checkout_task(&mut self) -> Result<(), Error> {
        if let Some(i) = self.tasks.state.selected() {
            let task = &self.tasks.items[i];
            
            // Determine the branch name to checkout
            let branch_name = if let Some(sprint_branch) = &task.sprint_branch {
                format!("{}-task-{}", sprint_branch, task.id)
            } else {
                // Fallback if no sprint branch is stored
                let sprint_branch = if self.current_branch.starts_with("sprint-") {
                    self.current_branch.clone()
                } else {
                    format!("sprint-{}", self.current_branch)
                };
                format!("{}-task-{}", sprint_branch, task.id)
            };
            
            // Checkout the task branch
            CheckoutCommand::execute(&branch_name)?;
            
            // Update current branch
            self.current_branch = branch_name;
        }
        Ok(())
    }
}

pub struct TaskListCommand {
    pub repo_path: String,
    pub args: Vec<String>,
}

impl TaskListCommand {
    pub fn execute(&self) -> Result<(), Error> {
        // Initialize managers
        let branch_manager = BranchMetadataManager::new(Path::new(&self.repo_path));
        let task_manager = CommitMetadataManager::new(Path::new(&self.repo_path));
        let sprint_manager = SprintManager::new(Path::new(&self.repo_path));
        
        // Get current branch
        let current_branch = branch_manager.get_current_branch()?;
        
        // Get all tasks
        let all_tasks = task_manager.list_all_tasks()?;
        
        // Find active sprint
        let active_sprint = branch_manager.find_active_sprint()?;
        let active_sprint_branch = if let Some((name, _)) = &active_sprint {
            if name.starts_with("sprint-") {
                Some(name.clone())
            } else {
                Some(format!("sprint-{}", name))
            }
        } else {
            None
        };
        
        // Determine current sprint from branch if possible
        let current_sprint_branch = if current_branch.starts_with("sprint-") && !current_branch.contains("-task-") {
            // Current branch is a sprint branch
            Some(current_branch.clone())
        } else if current_branch.contains("-task-") {
            // Extract sprint from task branch format: sprint-sprintX-task-TASKID
            let parts: Vec<&str> = current_branch.split("-task-").collect();
            if !parts.is_empty() {
                Some(parts[0].to_string())
            } else {
                active_sprint_branch.clone()
            }
        } else {
            active_sprint_branch.clone()
        };
        
        // Get tasks for the current sprint
        let mut current_sprint_tasks = Vec::new();
        if let Some(sprint_branch) = &current_sprint_branch {
            let sprint_tasks = sprint_manager.get_sprint_tasks(sprint_branch)?;
            
            // Map the sprint task IDs to the full task metadata
            for (id, _) in sprint_tasks {
                if let Ok(Some(task_meta)) = task_manager.get_task_metadata(&id) {
                    current_sprint_tasks.push(task_meta);
                }
            }
        }
        
        // Create task list for the UI with all tasks
        let mut task_infos = Vec::new();

        // Create a repository to access refs
        let repo = Repository::new(&self.repo_path)?;

        for task in &all_tasks {
            // Determine which sprint this task belongs to (if known)
            let sprint_ref_key = format!("refs/meta/tasksprint/{}", task.id);
            let sprint_branch = repo.refs.read_ref(&sprint_ref_key).ok().flatten();
            
            let task_info = TaskInfo {
                id: task.id.clone(),
                description: task.description.clone(),
                story_points: task.story_points,
                status: task.status.clone(),
                sprint_branch,
            };
            
            task_infos.push(task_info);
        }
        
        // Setup terminal UI
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        
        // Create app state
        let app = TaskApp::new(task_infos, current_branch);
        
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

    fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: TaskApp) -> io::Result<()> {
        loop {
            terminal.draw(|f| Self::ui(f, &mut app))?;
            
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Down => app.next_filtered_task(),
                    KeyCode::Up => app.previous_filtered_task(),
                    KeyCode::Enter => {
                        // Checkout the selected task
                        if let Err(e) = app.checkout_task() {
                            eprintln!("Error checking out task: {}", e);
                        } else {
                            return Ok(());  // Exit after successful checkout
                        }
                    },
                    KeyCode::Char('f') => {
                        // Toggle filter mode
                        app.next_filter_mode();
                    },
                    _ => {}
                }
            }
        }
    }
    
    fn ui<B: Backend>(f: &mut tui::Frame<B>, app: &mut TaskApp) {
        let size = f.size();
        
        // Create the outer block
        let block = Block::default()
            .title("AsheraFlow Task Manager")
            .borders(Borders::ALL);
        f.render_widget(block, size);
        
        // Filter tasks based on current filter mode
        let filtered_tasks: Vec<&TaskInfo> = app.tasks.items.iter()
            .filter(|task| {
                match app.filter_mode {
                    FilterMode::All => true,
                    FilterMode::CurrentSprint => {
                        if let Some(branch) = &task.sprint_branch {
                            branch == &app.current_branch || 
                            (app.current_branch.contains("-task-") && branch.contains(&app.current_branch.split("-task-").next().unwrap_or("")))
                        } else {
                            false
                        }
                    },
                    FilterMode::ActiveTasks => {
                        task.status == crate::core::commit_metadata::TaskStatus::InProgress
                    }
                }
            })
            .collect();
        
        // Create a mapping from filtered index to original index
        let index_map: Vec<usize> = app.tasks.items.iter().enumerate()
            .filter(|(_, task)| {
                match app.filter_mode {
                    FilterMode::All => true,
                    FilterMode::CurrentSprint => {
                        if let Some(branch) = &task.sprint_branch {
                            branch == &app.current_branch || 
                            (app.current_branch.contains("-task-") && branch.contains(&app.current_branch.split("-task-").next().unwrap_or("")))
                        } else {
                            false
                        }
                    },
                    FilterMode::ActiveTasks => {
                        task.status == crate::core::commit_metadata::TaskStatus::InProgress
                    }
                }
            })
            .map(|(i, _)| i)
            .collect();
        
        // Create list items for filtered tasks
        let items: Vec<ListItem> = filtered_tasks.iter().enumerate().map(|(filtered_idx, task)| {
            let original_idx = index_map[filtered_idx];
            let selected = app.tasks.state.selected() == Some(original_idx);
            
            // Format task status
            let status_str = match task.status {
                crate::core::commit_metadata::TaskStatus::Todo => "[TODO]",
                crate::core::commit_metadata::TaskStatus::InProgress => "[IN PROGRESS]",
                crate::core::commit_metadata::TaskStatus::Done => "[DONE]",
            };
            
            // Format task display
            let points_str = match task.story_points {
                Some(points) => format!("({}sp)", points),
                None => "".to_string(),
            };
            
            let task_text = format!("{} - {} {}", task.id, task.description, points_str);
            
            // Determine color based on task status
            let spans = match task.status {
                crate::core::commit_metadata::TaskStatus::Todo => {
                    Spans::from(vec![
                        Span::styled(status_str, Style::default().fg(Color::Red)),
                        Span::raw(" "),
                        Span::styled(task_text, Style::default()),
                    ])
                },
                crate::core::commit_metadata::TaskStatus::InProgress => {
                    Spans::from(vec![
                        Span::styled(status_str, Style::default().fg(Color::Yellow)),
                        Span::raw(" "),
                        Span::styled(task_text, Style::default()),
                    ])
                },
                crate::core::commit_metadata::TaskStatus::Done => {
                    Spans::from(vec![
                        Span::styled(status_str, Style::default().fg(Color::Green)),
                        Span::raw(" "),
                        Span::styled(task_text, Style::default()),
                    ])
                },
            };
            
            ListItem::new(vec![spans])
        }).collect();
        
        // Create the filter mode text
        let filter_text = match app.filter_mode {
            FilterMode::All => "Filter: All Tasks",
            FilterMode::CurrentSprint => "Filter: Current Sprint",
            FilterMode::ActiveTasks => "Filter: Active Tasks",
        };
        
        // Create help text
        let help_text = vec![
            Spans::from(vec![
                Span::raw("↑/↓: Navigate  "),
                Span::raw("Enter: Checkout Task  "),
                Span::raw("f: Change Filter  "),
                Span::raw("q: Quit  "),
                Span::styled(filter_text, Style::default().fg(Color::Yellow)),
            ]),
        ];
        
        let help_paragraph = tui::widgets::Paragraph::new(help_text)
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::NONE));
        
        // Create status text showing total tasks displayed vs. total tasks
        let status_text = vec![
            Spans::from(vec![
                Span::styled(
                    format!("Displaying {} of {} tasks", filtered_tasks.len(), app.tasks.items.len()),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
        ];
        
        let status_paragraph = tui::widgets::Paragraph::new(status_text)
            .style(Style::default())
            .block(Block::default().borders(Borders::NONE));
        
        // Create the list widget
        let list = List::new(items)
            .block(Block::default().title("Tasks").borders(Borders::NONE))
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        
        // Layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ].as_ref())
            .split(f.size());
        
        // Render status text at top
        f.render_widget(status_paragraph, chunks[0]);
        
        // Render the list with state
        let mut list_state = ListState::default();
        if let Some(selected) = app.tasks.state.selected() {
            // Map original index to filtered index
            let filtered_idx = index_map.iter().position(|&i| i == selected);
            list_state.select(filtered_idx);
        }
        f.render_stateful_widget(list, chunks[1], &mut list_state);
        
        // Render help text at bottom
        f.render_widget(help_paragraph, chunks[2]);
    }
} 