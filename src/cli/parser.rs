use crate::cli::args::{CliArgs, Command};
use crate::errors::error::Error;

pub struct CliParser;

impl CliParser {
    pub fn parse(args: Vec<String>) -> Result<CliArgs, Error> {
        if args.len() < 2 {
            return Err(Error::Generic("Usage: ash <command> [options]".to_string()));
        }

        let command = args[1].to_lowercase();
        let cli_args = match command.as_str() {
            "init" => CliArgs {
                command: Command::Init {
                    path: args.get(2).map(|s| s.to_owned()).unwrap_or(".".to_string()),
                },
            },
            "commit" => {
                let mut message = String::new();
                let mut i = 2;
                while i < args.len() {
                    if args[i] == "--message" || args[i] == "-m" {
                        if i + 1 < args.len() {
                            message = args[i + 1].to_owned();
                            break;
                        }
                    }
                    i += 1;
                }
                
                if message.is_empty() {
                    return Err(Error::Generic("Commit message is required. Use --message <msg> or -m <msg>".to_string()));
                }
                
                CliArgs {
                    command: Command::Commit {
                        message,
                    },
                }
            },
            "add" => {
                if args.len() < 3 {
                    return Err(Error::Generic("File path is required for add command".to_string()));
                }
                CliArgs {
                    command: Command::Add {
                        paths: args[2..].to_vec(),
                    },
                }
            },
            "status" => {
                // Check for --porcelain flag
                let porcelain = args.iter().skip(2).any(|arg| arg == "--porcelain");

                // Check for --color option
                let color = args.iter().skip(2).enumerate().find_map(|(i, arg)| {
                    if arg == "--color" && i + 1 < args.len() - 2 {
                        Some(args[i + 3].clone())
                    } else if arg.starts_with("--color=") {
                        Some(arg.split('=').nth(1).unwrap_or("auto").to_string())
                    } else {
                        None
                    }
                }).unwrap_or_else(|| "auto".to_string());
                
                CliArgs {
                    command: Command::Status {
                        porcelain,
                        color,
                    },
                }
            },
            "diff" => {
                // Parse diff command arguments
                let mut paths = Vec::new();
                let mut cached = false;
                
                // Check for --cached or --staged flag
                for arg in args.iter().skip(2) {
                    if arg == "--cached" || arg == "--staged" {
                        cached = true;
                    } else if !arg.starts_with("-") {
                        paths.push(arg.clone());
                    }
                }
                
                CliArgs {
                    command: Command::Diff {
                        paths,
                        cached,
                    },
                }
            },
            "branch" => {
                // Parse branch options
                let mut name = String::new();
                let mut start_point = None;
                let mut verbose = false;
                let mut delete = false;
                let mut force = false;
                
                // Process all arguments for options
                let mut i = 2;
                while i < args.len() {
                    let arg = &args[i];
                    match arg.as_str() {
                        "-v" | "--verbose" => {
                            verbose = true;
                            i += 1;
                        },
                        "-d" | "--delete" => {
                            delete = true;
                            i += 1;
                        },
                        "-f" | "--force" => {
                            force = true;
                            i += 1;
                        },
                        "-D" => {
                            delete = true;
                            force = true;
                            i += 1;
                        },
                        a if a.starts_with("-") => {
                            return Err(Error::Generic(format!("Unknown option: {}", a)));
                        },
                        _ => {
                            // If name is not set, this is the branch name
                            if name.is_empty() {
                                name = arg.clone();
                            } else if start_point.is_none() {
                                // If name is set but start_point isn't, this is the start point
                                start_point = Some(arg.clone());
                            }
                            i += 1;
                        }
                    }
                }
                
                // When listing branches (no name specified), name could be empty
                
                CliArgs {
                    command: Command::Branch {
                        name,
                        start_point,
                        verbose,
                        delete,
                        force
                    },
                }
            },
            "checkout" => {
                if args.len() < 3 {
                    return Err(Error::Generic("No checkout target specified".to_string()));
                }
                
                let target = args[2].clone();
                
                CliArgs {
                    command: Command::Checkout {
                        target,
                    },
                }
            },
            "log" => {
                // Parse log command options
                let mut revisions = Vec::new();
                let mut abbrev = false;
                let mut format = "medium".to_string();
                let mut patch = false;
                let mut decorate = "auto".to_string();
                
                // Process arguments
                let mut i = 2;
                while i < args.len() {
                    let arg = &args[i];
                    match arg.as_str() {
                        "--abbrev-commit" => {
                            abbrev = true;
                            i += 1;
                        },
                        "--no-abbrev-commit" => {
                            abbrev = false;
                            i += 1;
                        },
                        "--pretty" | "--format" => {
                            if i + 1 < args.len() {
                                format = args[i + 1].clone();
                                i += 2;
                            } else {
                                i += 1;
                            }
                        },
                        a if a.starts_with("--pretty=") || a.starts_with("--format=") => {
                            let value = arg.split('=').nth(1).unwrap_or("medium");
                            format = value.to_string();
                            i += 1;
                        },
                        "--oneline" => {
                            format = "oneline".to_string();
                            abbrev = true;
                            i += 1;
                        },
                        "-p" | "-u" | "--patch" => {
                            patch = true;
                            i += 1;
                        },
                        "-s" | "--no-patch" => {
                            patch = false;
                            i += 1;
                        },
                        "--decorate" => {
                            decorate = "short".to_string();
                            i += 1;
                        },
                        a if a.starts_with("--decorate=") => {
                            let value = arg.split('=').nth(1).unwrap_or("short");
                            decorate = value.to_string();
                            i += 1;
                        },
                        "--no-decorate" => {
                            decorate = "no".to_string();
                            i += 1;
                        },
                        a if a.starts_with("-") => {
                            return Err(Error::Generic(format!("Unknown option: {}", a)));
                        },
                        _ => {
                            // This is a revision specifier
                            revisions.push(arg.clone());
                            i += 1;
                        }
                    }
                }
                
                CliArgs {
                    command: Command::Log {
                        revisions,
                        abbrev,
                        format,
                        patch,
                        decorate,
                    },
                }
            },
            _ => CliArgs {
                command: Command::Unknown { name: command },
            },
        };

        Ok(cli_args)
    }

    pub fn format_help() -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            "Usage: ash <command> [options]",
            "Commands:",
            "  init [path]                      Initialize a new repository",
            "  commit <message>                 Commit changes to the repository",
            "  add <paths...>                   Add file contents to the index",
            "  status [--porcelain] [--color=<when>]   Show the working tree status",
            "         --porcelain               Machine-readable output",
            "         --color=<when>            Colorize output (always|auto|never)",
            "  diff [--cached] [paths...]      Show changes between commits, commit and working tree, etc",
            "  branch <n> [start-point]        Create a new branch",
            "         <n>                       Name of the branch to create",
            "         [start-point]             Revision to start the branch at (defaults to HEAD)",
            "  checkout <target>                Switch branches or restore working tree files"
        )
    }
}

