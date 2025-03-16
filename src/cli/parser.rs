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
                
                CliArgs {
                    command: Command::Status {
                        porcelain,
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
            "{}\n{}\n{}\n{}\n{}\n{}\n{}",
            "Usage: ash <command> [options]",
            "Commands:",
            "  init [path]           Initialize a new repository",
            "  commit <message>      Commit changes to the repository",
            "  add <paths...>        Add file contents to the index",
            "  status [--porcelain]  Show the working tree status",
            "  validate              Validate repository health and integrity"
        )
    }
}
