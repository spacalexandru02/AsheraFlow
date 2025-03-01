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
                if args.len() < 3 {
                    return Err(Error::Generic("Commit message is required".to_string()));
                }
                CliArgs {
                    command: Command::Commit {
                        message: args[2].to_owned(),
                    },
                }
            }
            _ => CliArgs {
                command: Command::Unknown { name: command },
            },
        };

        Ok(cli_args)
    }

    pub fn format_help() -> String {
        format!(
            "{}\n{}\n{}\n{}",
            "Usage: ash <command> [options]",
            "Commands:",
            "  init [path]    Initialize a new repository",
            "  commit <message>  Commit changes to the repository"
        )
    }
}