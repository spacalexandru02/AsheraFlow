use std::path::Path;
use std::collections::HashMap;
use std::io;
use std::time::{Duration as StdDuration, Instant};
use chrono::{NaiveDateTime, Utc, Duration, NaiveDate, Datelike};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Span, Spans},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Terminal,
};

use crate::errors::error::Error;
use crate::core::branch_metadata::BranchMetadataManager;
use crate::core::sprint::sprint::{SprintManager, TaskStatus};

pub struct SprintBurndownCommand;

struct BurndownData {
    sprint_name: String,
    total_points: u32,
    days_passed: usize,
    days_remaining: usize,
    total_days: usize,
    start_date: NaiveDate,
    end_date: NaiveDate,
    daily_progress: Vec<(usize, u32)>,
    ideal_progress: Vec<(f64, f64)>,
    actual_progress: Vec<(f64, f64)>,
}

impl SprintBurndownCommand {
    pub fn execute(sprint_name: Option<&str>) -> Result<(), Error> {
        // Initialize the repository path
        let root_path = Path::new(".");
        let git_path = root_path.join(".ash");
        
        // Verify .ash directory exists
        if !git_path.exists() {
            return Err(Error::Generic("Not an ash repository: .ash directory not found".into()));
        }
        
        // Create branch metadata manager
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
        
        // Get tasks for this sprint
        let tasks = match sprint_manager.get_sprint_tasks(&branch_name) {
            Ok(tasks) => tasks,
            Err(e) => return Err(Error::Generic(format!("Failed to get tasks: {}", e))),
        };
        
        // Get total story points for the sprint
        let mut total_story_points = 0;
        for (_, task) in &tasks {
            if let Some(points) = task.story_points {
                total_story_points += points;
            }
        }
        
        // Calculate the start/end dates and current day
        let start_date = NaiveDateTime::from_timestamp_opt(sprint_metadata.start_timestamp as i64, 0)
            .unwrap()
            .date();
        let end_date = NaiveDateTime::from_timestamp_opt(sprint_metadata.end_timestamp() as i64, 0)
            .unwrap()
            .date();
        let current_date = Utc::now().naive_utc().date();
        
        // Calculate number of days in sprint
        let total_days = (end_date - start_date).num_days() as usize;
        let days_passed = std::cmp::min((current_date - start_date).num_days() as usize, total_days);
        let days_remaining = total_days - days_passed;
        
        // Get daily completion progress
        let daily_progress = get_daily_progress(&tasks, start_date, total_days);
        
        // Create ideal and actual data points for the chart
        let (ideal_progress, actual_progress) = create_chart_data(
            total_story_points,
            total_days,
            start_date,
            &daily_progress
        );
        
        // Prepare burndown data for visualization
        let burndown_data = BurndownData {
            sprint_name: sprint_metadata.name.clone(),
            total_points: total_story_points,
            days_passed,
            days_remaining,
            total_days,
            start_date,
            end_date,
            daily_progress,
            ideal_progress,
            actual_progress,
        };
        
        // Show interactive UI
        show_burndown_chart(burndown_data)?;
        
        Ok(())
    }
}

fn get_daily_progress(tasks: &HashMap<String, crate::core::sprint::sprint::Task>, 
                     start_date: NaiveDate, 
                     total_days: usize) -> Vec<(usize, u32)> {
    let mut daily_completed = vec![0; total_days + 1];
    
    let sprint_end_date = start_date + Duration::days(total_days as i64);
    
    for (_, task) in tasks {
        if task.status == TaskStatus::Done && task.completed_at.is_some() {
            let completed_date = NaiveDateTime::from_timestamp_opt(task.completed_at.unwrap() as i64, 0)
                .unwrap()
                .date();
            
            // Check if the task was completed during the sprint period
            if completed_date >= start_date && completed_date <= sprint_end_date {
                let day_idx = (completed_date - start_date).num_days() as usize;
                if day_idx <= total_days {
                    if let Some(points) = task.story_points {
                        daily_completed[day_idx] += points;
                    } else {
                        // Default 1 point if not specified
                        daily_completed[day_idx] += 1;
                    }
                }
            }
        }
    }
    
    // Convert to cumulative completion
    let mut result = Vec::new();
    let mut cumulative = 0;
    
    for (day, points) in daily_completed.iter().enumerate() {
        cumulative += points;
        result.push((day, cumulative));
    }
    
    result
}

fn create_chart_data(total_points: u32, 
                   total_days: usize,
                   start_date: NaiveDate,
                   daily_progress: &[(usize, u32)]) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    // Calculate ideal progress (linear downward trend)
    let mut ideal_progress = Vec::new();
    
    // Calculate working days (exclude weekends)
    let mut working_days = Vec::new();
    for d in 0..=total_days {
        let date = start_date + Duration::days(d as i64);
        if date.weekday().number_from_monday() < 6 {
            working_days.push(d);
        }
    }
    
    let total_working_days = working_days.len();
    let points_per_working_day = if total_working_days > 0 {
        total_points as f64 / total_working_days as f64
    } else {
        0.0
    };
    
    for d in 0..=total_days {
        let day = d as f64;
        
        // Calculate working days up to current day
        let working_days_so_far = working_days.iter().filter(|&&wd| wd <= d).count();
        
        // Calculate ideal remaining points (subtracting ideal progress)
        let ideal_remaining = total_points as f64 - (working_days_so_far as f64 * points_per_working_day);
        
        // Add the point to the chart
        ideal_progress.push((day, ideal_remaining.max(0.0)));
    }
    
    // Calculate actual progress
    let mut actual_progress = Vec::new();
    for d in 0..=total_days {
        let day = d as f64;
        
        // Calculate actual points remaining (total - completed at this day)
        let actual_points = if d < daily_progress.len() {
            total_points - daily_progress[d].1
        } else {
            // If we don't have data for this day, use the last known value
            if !daily_progress.is_empty() {
                total_points - daily_progress.last().unwrap().1
            } else {
                total_points // No progress yet
            }
        };
        
        actual_progress.push((day, actual_points as f64));
    }
    
    (ideal_progress, actual_progress)
}

fn show_burndown_chart(data: BurndownData) -> Result<(), Error> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state
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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, data: BurndownData) -> io::Result<()> {
    let mut scale_factor = 1.0; // Zoom factor for chart
    
    loop {
        terminal.draw(|f| ui(f, &data, scale_factor))?;
        
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    scale_factor *= 1.1;
                },
                KeyCode::Char('-') => {
                    scale_factor /= 1.1;
                    if scale_factor < 0.1 {
                        scale_factor = 0.1;
                    }
                },
                KeyCode::Esc => {
                    scale_factor = 1.0; // Reset zoom
                },
                _ => {}
            }
        }
    }
}

fn ui<B: Backend>(f: &mut tui::Frame<B>, data: &BurndownData, scale_factor: f32) {
    let size = f.size();
    
    // Layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(10),   // Chart
            Constraint::Length(3), // Legend
        ].as_ref())
        .split(size);
    
    // Title
    let days_remaining_text = if data.days_remaining > 0 {
        format!("{} days remaining", data.days_remaining)
    } else {
        "Sprint completed".to_string()
    };
    
    let title = vec![
        Spans::from(vec![
            Span::styled(
                format!("Sprint \"{}\" Burndown Chart", data.sprint_name),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            ),
        ]),
        Spans::from(vec![
            Span::raw(format!(
                "{} - {} | {}/{} days | {} story points",
                data.start_date.format("%Y-%m-%d"),
                data.end_date.format("%Y-%m-%d"),
                data.days_passed,
                data.total_days,
                data.total_points
            )),
        ]),
    ];
    
    let title_paragraph = Paragraph::new(title)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::NONE));
    
    f.render_widget(title_paragraph, chunks[0]);
    
    // Burndown Chart
    let max_y = data.total_points as f64 * 1.05; // Add some padding
    
    // Create data for vertical weekend markers
    let weekend_markers: Vec<(f64, f64)> = (0..=data.total_days)
        .filter(|&d| {
            let date = data.start_date + Duration::days(d as i64);
            date.weekday().number_from_monday() >= 6
        })
        .flat_map(|d| {
            vec![(d as f64, 0.0), (d as f64, max_y)]
        })
        .collect();
    
    // Create vertical marker for today
    let today_marker = vec![(data.days_passed as f64, 0.0), (data.days_passed as f64, max_y)];
    
    // Adjust chart data based on scale factor
    let max_x = data.total_days as f64 * scale_factor as f64;
    
    let datasets = vec![
        Dataset::default()
            .name("Weekend")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::DarkGray))
            .data(&weekend_markers),
        Dataset::default()
            .name("Today")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Magenta))
            .data(&today_marker),
        Dataset::default()
            .name("Ideal Progress")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Green))
            .data(&data.ideal_progress),
        Dataset::default()
            .name("Actual Progress")
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .data(&data.actual_progress)
    ];
    
    // Calculate percentage complete
    let completed_points = if !data.actual_progress.is_empty() {
        data.total_points as f64 - data.actual_progress.last().unwrap().1
    } else {
        0.0
    };
    let completion_percentage = (completed_points / data.total_points as f64 * 100.0).round();
    
    // Create chart
    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(format!("Burndown ({}% complete)", completion_percentage))
                .borders(Borders::ALL)
        )
        .x_axis(
            Axis::default()
                .title("Days")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_x])
                .labels(vec![
                    Span::styled("0", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{}", data.total_days / 2),
                        Style::default().fg(Color::Gray)
                    ),
                    Span::styled(
                        format!("{}", data.total_days),
                        Style::default().fg(Color::Gray)
                    ),
                ])
        )
        .y_axis(
            Axis::default()
                .title("Story Points Remaining")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_y])
                .labels(vec![
                    Span::styled("0", Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{}", (max_y / 2.0).round()),
                        Style::default().fg(Color::Gray)
                    ),
                    Span::styled(
                        format!("{}", data.total_points),
                        Style::default().fg(Color::Gray)
                    ),
                ])
        );
    
    f.render_widget(chart, chunks[1]);
    
    // Legend for controls
    let legend = vec![
        Spans::from(vec![
            Span::styled("Controls: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw("q: Quit | +/=: Zoom In | -: Zoom Out | Esc: Reset Zoom"),
        ]),
        Spans::from(vec![
            Span::styled("Legend: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("■ ", Style::default().fg(Color::Green)),
            Span::raw("Ideal Progress | "),
            Span::styled("■ ", Style::default().fg(Color::Yellow)),
            Span::raw("Actual Progress | "),
            Span::styled("■ ", Style::default().fg(Color::Magenta)),
            Span::raw("Today | "),
            Span::styled("■ ", Style::default().fg(Color::DarkGray)),
            Span::raw("Weekend"),
        ]),
    ];
    
    let legend_paragraph = Paragraph::new(legend)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::NONE));
    
    f.render_widget(legend_paragraph, chunks[2]);
} 