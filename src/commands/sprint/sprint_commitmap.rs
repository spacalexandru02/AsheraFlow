use std::path::Path;
use std::collections::HashMap;
use std::io;
use std::time::{Duration as StdDuration, Instant};
use chrono::{NaiveDateTime, Utc, Duration, NaiveDate, Datelike, Timelike};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    style::Color as CrosstermColor,
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
    Terminal,
    buffer::Buffer,
};

use crate::errors::error::Error;
use crate::core::branch_metadata::BranchMetadataManager;
use crate::core::sprint::sprint::SprintManager;
use crate::core::repository::repository::Repository;
use crate::core::database::database::Database;
use crate::core::database::commit::Commit;

pub struct SprintCommitMapCommand;

struct CommitMapData {
    sprint_name: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    commit_heatmap: HashMap<NaiveDate, Vec<u32>>, // Map de date către activitate orară
    valid_days: Vec<NaiveDate>,  // Zilele valide din sprint
    scroll_offset: usize,
    scroll_animation_frame: u8,
    last_scroll_time: Instant,
}

impl SprintCommitMapCommand {
    pub fn execute(sprint_name: Option<&str>) -> Result<(), Error> {
        // Initialize the repository path
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository: .ash directory not found".into()));
        }
        
        // Create repository, branch manager and sprint manager
        let db_path = git_path.join("objects");
        let database = Database::new(db_path);
        let mut repository = Repository::new(".")?;
        let branch_manager = BranchMetadataManager::new(root_path);
        let sprint_manager = SprintManager::new(root_path);
        
        // Find the target sprint (active sprint if none specified)
        let (branch_name, sprint_metadata) = match sprint_name {
            Some(name) => {
                // Find specific sprint by name
                let branch_name = format!("sprint-{}", name.to_lowercase());
                match branch_manager.get_sprint_metadata(&branch_name)? {
                    Some(meta) => (branch_name, meta),
                    None => return Err(Error::Generic(format!("Sprint '{}' not found", name))),
                }
            },
            None => {
                // Find active sprint
                match branch_manager.find_active_sprint()? {
                    Some((branch, meta)) => (branch, meta),
                    None => return Err(Error::Generic("No active sprint found".into())),
                }
            }
        };
        
        // Calculate the start/end dates
        let start_date = NaiveDateTime::from_timestamp_opt(sprint_metadata.start_timestamp as i64, 0)
            .unwrap()
            .date();
        let end_date = NaiveDateTime::from_timestamp_opt(sprint_metadata.end_timestamp() as i64, 0)
            .unwrap()
            .date();
        
        // Get all commits in the sprint branch
        let commits = get_commits_in_branch(&mut repository, &branch_name)?;
        
        // Create commit heatmap pentru fiecare zi din sprint (map data -> activitate orară)
        let mut commit_heatmap = HashMap::new();
        let mut current_date = start_date;
        let mut valid_days = Vec::new();
        
        while current_date <= end_date {
            commit_heatmap.insert(current_date, vec![0; 24]);
            valid_days.push(current_date);
            current_date = current_date.succ_opt().unwrap();
        }
        
        // Contorizează commit-urile
        for commit in commits {
            let commit_time = NaiveDateTime::from_timestamp_opt(commit.committer.timestamp.timestamp(), 0).unwrap();
            let commit_date = commit_time.date();
            
            // Only count commits within the sprint period
            if commit_date >= start_date && commit_date <= end_date {
                // Get hour of day (0-23)
                let hour_idx = commit_time.hour() as usize;
                
                // Incrementează numărul de commit-uri pentru această zi și oră
                if let Some(day_data) = commit_heatmap.get_mut(&commit_date) {
                    day_data[hour_idx] += 1;
                }
            }
        }
        
        // Prepare commit map data for visualization
        let commit_map_data = CommitMapData {
            sprint_name: sprint_metadata.name.clone(),
            start_date,
            end_date,
            commit_heatmap,
            valid_days,  // Adăugăm lista de zile valide
            scroll_offset: 0,
            scroll_animation_frame: 0,
            last_scroll_time: Instant::now(),
        };
        
        // Show terminal-based commit map visualization
        show_commit_map(commit_map_data)?;
        
        Ok(())
    }
}

fn show_commit_map(data: CommitMapData) -> Result<(), Error> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state and run
    let res = run_app(&mut terminal, data);
    
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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut data: CommitMapData) -> io::Result<()> {
    let valid_count = data.valid_days.len();
    let tick_rate = StdDuration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        // Desenează UI
        terminal.draw(|f| ui(f, &mut data, valid_count))?;

        // Calculează visible_days corect pentru commit map (exclude titlu(3), legendă(3), header+margin(4), top/bottom border(2))
        let size = terminal.size()?;
        let visible_days = (size.height as usize).saturating_sub(17);
        let max_scroll = valid_count.saturating_sub(visible_days);

        let now = Instant::now();
        let timeout = tick_rate
            .checked_sub(now.duration_since(last_tick))
            .unwrap_or_else(|| StdDuration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => {
                        data.scroll_offset = (data.scroll_offset + 1).min(max_scroll);
                        data.last_scroll_time = Instant::now();
                        data.scroll_animation_frame = 0;
                    },
                    KeyCode::Up | KeyCode::Char('k') => {
                        data.scroll_offset = data.scroll_offset.saturating_sub(1);
                        data.last_scroll_time = Instant::now();
                        data.scroll_animation_frame = 0;
                    },
                    KeyCode::PageDown => {
                        data.scroll_offset = (data.scroll_offset + visible_days).min(max_scroll);
                        data.last_scroll_time = Instant::now();
                        data.scroll_animation_frame = 0;
                    },
                    KeyCode::PageUp => {
                        data.scroll_offset = data.scroll_offset.saturating_sub(visible_days);
                        data.last_scroll_time = Instant::now();
                        data.scroll_animation_frame = 0;
                    },
                    KeyCode::Home => {
                        data.scroll_offset = 0;
                        data.last_scroll_time = Instant::now();
                        data.scroll_animation_frame = 0;
                    },
                    KeyCode::End => {
                        data.scroll_offset = max_scroll;
                        data.last_scroll_time = Instant::now();
                        data.scroll_animation_frame = 0;
                    },
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
            if data.last_scroll_time.elapsed() < StdDuration::from_millis(500) {
                data.scroll_animation_frame = (data.scroll_animation_frame + 1) % 4;
            }
        }
    }
}

fn ui<B: Backend>(f: &mut tui::Frame<B>, data: &mut CommitMapData, valid_count: usize) {
    let size = f.size();
    
    // Layout principal
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),   // Title
            Constraint::Min(10),     // Content (headers + commit map)
            Constraint::Length(3),   // Legend
        ].as_ref())
        .split(size);
    
    // Layout pentru conținut (headerul cu ore + commit map)
    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),   // Header cu ore (static)
            Constraint::Min(8),      // Commit map scrollabil
        ].as_ref())
        .split(chunks[1]);
    
    // Title
    let title = vec![
        Spans::from(vec![
            Span::styled(
                format!("Commit Activity (Sprint \"{}\")", data.sprint_name),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            ),
        ]),
        Spans::from(vec![
            Span::raw(format!(
                "From {} to {} ({} days)",
                data.start_date.format("%Y-%m-%d"),
                data.end_date.format("%Y-%m-%d"),
                valid_count
            )),
        ]),
    ];
    
    let title_paragraph = Paragraph::new(title)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::NONE));
    
    f.render_widget(title_paragraph, chunks[0]);
    
    // Headerul cu orele (care rămâne persistent)
    let hour_header = create_hour_header();
    let hour_header_widget = Paragraph::new(hour_header)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT).title("Hours"));
    
    f.render_widget(hour_header_widget, content_chunks[0]);
    
    // Lista de zile (scrollabilă)
    let commit_map = create_commit_map_content(data, 
        content_chunks[1].height.saturating_sub(2) as usize);
    
    let commit_map_widget = Paragraph::new(commit_map)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT).title("Daily Commit Activity"))
        .scroll((data.scroll_offset as u16, 0));
    
    f.render_widget(commit_map_widget, content_chunks[1]);
    
    // Desenarea barei de scroll ca un text widget
    let scrollbar_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ].as_ref())
        .split(content_chunks[1]);
    
    // Calculăm poziția scrollbar-ului
    let scroll_area_height = content_chunks[1].height.saturating_sub(2) as usize; // -2 pentru borders
    let total_items = valid_count;
    
    if total_items > scroll_area_height && scroll_area_height > 0 {
        let scroll_ratio = scroll_area_height as f64 / total_items as f64;
        let scrollbar_size = (scroll_area_height as f64 * scroll_ratio).max(1.0) as usize;
        
        let top_item = data.scroll_offset;
        let scroll_position = if total_items <= scrollbar_size {
            0
        } else {
            ((top_item as f64 / (total_items - scroll_area_height) as f64) 
                * (scroll_area_height - scrollbar_size) as f64) as usize
        };
        
        // Creăm liniile pentru scrollbar
        let mut scrollbar_lines = Vec::new();
        for i in 0..scroll_area_height {
            let in_scrollbar = i >= scroll_position && i < scroll_position + scrollbar_size;
            
            let scroll_char = if in_scrollbar {
                // Animația pentru scrollbar
                match data.scroll_animation_frame {
                    0 => "█",
                    1 => "▓", 
                    2 => "▒",
                    _ => "░",
                }
            } else {
                "│" // Track
            };
            
            let style = if in_scrollbar {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            
            scrollbar_lines.push(Spans::from(vec![
                Span::styled(scroll_char, style)
            ]));
        }
        
        // Creează scrollbar widget
        let scrollbar_widget = Paragraph::new(scrollbar_lines)
            .style(Style::default())
            .block(Block::default().borders(Borders::NONE));
        
        f.render_widget(scrollbar_widget, scrollbar_area[1]);
    }
    
    // Legend
    let legend = vec![
        Spans::from(vec![
            Span::raw("Legend: "),
            Span::raw("│  │ = 0    "),
            Span::styled("│░░│ = 1-3    ", Style::default().fg(Color::Gray)),
            Span::styled("│██│ = 4+", Style::default().fg(Color::White).bg(Color::DarkGray)),
            Span::styled("   ↑↓: Scroll   Home/End: Top/Bottom", Style::default().fg(Color::Yellow)),
        ]),
        Spans::from(vec![
            Span::styled("Press 'q' to exit", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
    ];
    
    let legend_widget = Paragraph::new(legend)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::NONE));
    
    f.render_widget(legend_widget, chunks[2]);
}

// Refactor create_hour_header to produce fixed 3-char columns
fn create_hour_header() -> Vec<Spans<'static>> {
    let mut lines = Vec::new();

    // First indent for date column
    let indent = "           "; // 11 spaces
    let mut header_spans = Vec::new();
    header_spans.push(Span::raw(indent));

    // For each two-hour block, center the label in 3 chars
    for hour in (0..24).step_by(2) {
        let label = format!("{:02}", hour);
        // center in width 3
        let cell = format!("{:^3}", label);
        header_spans.push(Span::raw(cell));
    }
    lines.push(Spans::from(header_spans));

    lines
}

// Funcția create_commit_map_content pentru a alinia coloanele de date corect
fn create_commit_map_content(data: &CommitMapData, visible_height: usize) -> Vec<Spans> {
    let mut lines = Vec::new();
    
    // Bordura de sus a tabelului trebuie să aibă exact 12 secțiuni pentru orele 00-22
    lines.push(Spans::from("          ┌──┬──┬──┬──┬──┬──┬──┬──┬──┬──┬──┬──┐"));
    
    // Folosim doar zilele valide pentru a evita scroll-ul prea mult
    for date in &data.valid_days {
        let day_data = &data.commit_heatmap[date];
        
        // Format date as "DD-MM Ddd"
        let weekday_name = match date.weekday().number_from_monday() {
            1 => "Mon",
            2 => "Tue", 
            3 => "Wed",
            4 => "Thu",
            5 => "Fri",
            6 => "Sat",
            7 => "Sun",
            _ => "???"
        };
        
        let mut day_spans = Vec::new();
        day_spans.push(Span::raw(format!("{:02}-{:02} {} │", date.day(), date.month(), weekday_name)));
        
        // Add commit activity cells for this day
        for hour in (0..24).step_by(2) {
            // Combine adjacent hours for more compact display
            let count = day_data[hour] + 
                       if hour + 1 < 24 { day_data[hour + 1] } else { 0 };
            
            let cell = match count {
                0 => Span::raw("  │"),
                1..=3 => Span::styled("░░│", Style::default().fg(Color::Gray)),
                _ => Span::styled("██│", Style::default().fg(Color::White).bg(Color::DarkGray)),
            };
            
            day_spans.push(cell);
        }
        
        lines.push(Spans::from(day_spans));
    }
    
    // Bordura de jos a tabelului trebuie să coincidă cu cea de sus
    lines.push(Spans::from("          └──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┘"));
    
    lines
}

// Helper function to get all commits in a branch
fn get_commits_in_branch(repository: &mut Repository, branch: &str) -> Result<Vec<Commit>, Error> {
    let mut commits = Vec::new();
    
    // Get the reference to the branch
    if let Some(current_oid) = repository.refs.read_ref(branch)? {
        let mut current = current_oid;
        
        while !current.is_empty() {
            if let Ok(object) = repository.database.load(&current) {
                if let Some(commit) = object.as_any().downcast_ref::<Commit>() {
                    // Add a clone of the commit to our list
                    commits.push(commit.clone());
                    
                    // Move to parent commit
                    if let Some(parent) = commit.get_parent() {
                        current = parent.to_string();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    
    Ok(commits)
} 