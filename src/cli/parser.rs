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
                if args.len() < 3 {
                    return Err(Error::Generic("Branch name is required".to_string()));
                }
                
                let name = args[2].clone();
                let start_point = if args.len() > 3 {
                    Some(args[3].clone())
                } else {
                    None
                };
                
                CliArgs {
                    command: Command::Branch {
                        name,
                        start_point,
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

