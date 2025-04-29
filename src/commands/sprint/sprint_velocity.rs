use std::path::Path;
use std::collections::HashMap;
use std::io;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Axis, BarChart, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Terminal,
};

use crate::errors::error::Error;
use crate::core::branch_metadata::BranchMetadataManager;
use crate::core::sprint::sprint::{SprintManager, TaskStatus};

pub struct SprintVelocityCommand;

struct SprintVelocityData {
    sprints: Vec<String>,                // Sprint names
    planned_points: Vec<u32>,            // Planned points per sprint
    completed_points: Vec<u32>,          // Completed points per sprint
    completion_rates: Vec<f64>,          // Percentage of completion per sprint
    avg_velocity: f64,                   // Average velocity across all sprints
}

impl SprintVelocityCommand {
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
        let sprint_manager = SprintManager::new(root_path);
        
        // Find all sprint branches
        let all_sprint_branches = branch_manager.get_all_sprints()?;
        
        if all_sprint_branches.is_empty() {
            println!("No sprints found in the repository.");
            return Ok(());
        }
        
        // Limit to last 10 sprints for display (increased from 5 for better visualization)
        let sprint_count = all_sprint_branches.len().min(10);
        let sprint_branches = if all_sprint_branches.len() > 10 {
            // Take the last 10 sprint branches (most recent ones)
            all_sprint_branches.iter()
                .skip(all_sprint_branches.len() - 10)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            all_sprint_branches
        };
        
        // Collect sprint data
        let mut sprints = Vec::with_capacity(sprint_count);
        let mut planned_points = Vec::with_capacity(sprint_count);
        let mut completed_points = Vec::with_capacity(sprint_count);
        let mut completion_rates = Vec::with_capacity(sprint_count);
        
        for (branch_name, sprint_metadata) in &sprint_branches {
            // Get tasks for this sprint
            if let Ok(tasks) = sprint_manager.get_sprint_tasks(branch_name) {
                let mut planned = 0;
                let mut completed = 0;
                
                for (_, task) in &tasks {
                    if let Some(points) = task.story_points {
                        planned += points;
                        
                        if task.status == TaskStatus::Done {
                            completed += points;
                        }
                    }
                }
                
                let completion_rate = if planned > 0 {
                    (completed as f64 / planned as f64) * 100.0
                } else {
                    0.0
                };
                
                sprints.push(sprint_metadata.name.clone());
                planned_points.push(planned);
                completed_points.push(completed);
                completion_rates.push(completion_rate);
            }
        }
        
        // Calculate average velocity
        let total_completed: u32 = completed_points.iter().sum();
        let avg_velocity = if !completed_points.is_empty() {
            total_completed as f64 / completed_points.len() as f64
        } else {
            0.0
        };
        
        // Create velocity data for visualization
        let velocity_data = SprintVelocityData {
            sprints,
            planned_points,
            completed_points,
            completion_rates,
            avg_velocity,
        };
        
        // Show interactive UI
        show_velocity_chart(velocity_data)?;
        
        Ok(())
    }
}

fn show_velocity_chart(data: SprintVelocityData) -> Result<(), Error> {
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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, data: SprintVelocityData) -> io::Result<()> {
    let mut view_mode = 0; // 0 = Bar Chart, 1 = Line Chart
    
    loop {
        terminal.draw(|f| ui(f, &data, view_mode))?;
        
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('v') => {
                    // Toggle view mode between Bar and Line chart
                    view_mode = (view_mode + 1) % 2;
                },
                _ => {}
            }
        }
    }
}

fn ui<B: Backend>(f: &mut tui::Frame<B>, data: &SprintVelocityData, view_mode: u8) {
    let size = f.size();
    
    // Layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(12),    // Chart
            Constraint::Length(3),  // Legend/Stats
        ].as_ref())
        .split(size);
    
    // Title
    let title = vec![
        Spans::from(vec![
            Span::styled(
                "Sprint Velocity Chart".to_string(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            ),
        ]),
        Spans::from(vec![
            Span::raw(format!(
                "Average Velocity: {:.1} points per sprint",
                data.avg_velocity
            )),
        ]),
    ];
    
    let title_paragraph = Paragraph::new(title)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::NONE));
    
    f.render_widget(title_paragraph, chunks[0]);
    
    // Prepare data for the view
    if view_mode == 0 {
        // Bar Chart View
        let barchart_data: Vec<(&str, u64)> = data.sprints.iter().zip(data.completed_points.iter())
            .map(|(name, points)| (name.as_str(), *points as u64))
            .collect();
            
        let max_value = data.planned_points.iter().max().cloned().unwrap_or(0) as u64;
            
        let barchart = BarChart::default()
            .block(Block::default()
                .title("Completed Story Points per Sprint")
                .borders(Borders::ALL))
            .bar_width(7)
            .bar_gap(2)
            .bar_style(Style::default().fg(Color::Yellow))
            .value_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
            .label_style(Style::default().fg(Color::Cyan))
            .data(&barchart_data)
            .max(max_value);
            
        f.render_widget(barchart, chunks[1]);
    } else {
        // Line Chart View - Show completion trend
        let mut velocity_data = Vec::new();
        let mut completion_rate_data = Vec::new();
        let mut planned_data = Vec::new();
        
        for (i, ((completed, rate), planned)) in data.completed_points.iter()
            .zip(data.completion_rates.iter())
            .zip(data.planned_points.iter())
            .enumerate() {
            velocity_data.push((i as f64, *completed as f64));
            completion_rate_data.push((i as f64, *rate));
            planned_data.push((i as f64, *planned as f64));
        }
        
        let datasets = vec![
            Dataset::default()
                .name("Planned Points")
                .marker(tui::symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Blue))
                .data(&planned_data),
            Dataset::default()
                .name("Completed Points")
                .marker(tui::symbols::Marker::Dot)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .data(&velocity_data),
            Dataset::default()
                .name("Completion Rate %")
                .marker(tui::symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Green))
                .data(&completion_rate_data),
        ];
        
        // Find max values for scaling
        let max_y = data.planned_points.iter().max().cloned().unwrap_or(0) as f64 * 1.1;
        
        // X-axis labels (sprint names)
        let x_labels: Vec<Span> = data.sprints.iter().enumerate()
            .map(|(i, name)| {
                Span::styled(
                    i.to_string(),
                    Style::default().fg(Color::Gray)
                )
            })
            .collect();
        
        let chart = Chart::new(datasets)
            .block(
                Block::default()
                    .title("Sprint Velocity Trend")
                    .borders(Borders::ALL)
            )
            .x_axis(
                Axis::default()
                    .title("Sprints")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, (data.sprints.len() - 1) as f64])
                    .labels(x_labels)
            )
            .y_axis(
                Axis::default()
                    .title("Story Points / Percentage")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, max_y.max(100.0)])
                    .labels(vec![
                        Span::styled("0", Style::default().fg(Color::Gray)),
                        Span::styled(
                            format!("{}", (max_y / 2.0).round()),
                            Style::default().fg(Color::Gray)
                        ),
                        Span::styled(
                            format!("{}", max_y.round()),
                            Style::default().fg(Color::Gray)
                        ),
                    ])
            );
        
        f.render_widget(chart, chunks[1]);
    }
    
    // Sprint Legend & Stats
    let stats = if !data.sprints.is_empty() {
        let total_completed = data.completed_points.iter().sum::<u32>();
        let total_planned = data.planned_points.iter().sum::<u32>();
        let avg_completion = if total_planned > 0 {
            (total_completed as f64 / total_planned as f64) * 100.0
        } else {
            0.0
        };
        
        vec![
            Spans::from(vec![
                Span::styled("Controls: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw("q: Quit | v: Toggle View (Bar/Line)"),
            ]),
            Spans::from(vec![
                Span::styled("Legend: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("■ Planned Points ", Style::default().fg(Color::Blue)),
                Span::raw("| "),
                Span::styled("■ Completed Points ", Style::default().fg(Color::Yellow)),
                Span::raw("| "),
                Span::styled("■ Completion Rate % ", Style::default().fg(Color::Green)),
            ]),
            Spans::from(vec![
                Span::styled("Stats: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw(format!("Total Completed: {}/{} points", total_completed, total_planned)),
                Span::raw(" | "),
                Span::styled(
                    format!("Avg. Completion: {:.1}%", avg_completion),
                    if avg_completion >= 80.0 {
                        Style::default().fg(Color::Green)
                    } else if avg_completion >= 50.0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Red)
                    }
                )
            ]),
        ]
    } else {
        vec![
            Spans::from(vec![
                Span::styled("No sprint data available", Style::default().fg(Color::Red))
            ])
        ]
    };
    
    let stats_paragraph = Paragraph::new(stats)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::NONE));
    
    f.render_widget(stats_paragraph, chunks[2]);
}