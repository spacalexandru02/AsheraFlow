use crate::cli::args::{CliArgs, Command};
use crate::errors::error::Error;

pub struct CliParser;

impl CliParser {
    pub fn parse(args: Vec<String>) -> Result<CliArgs, Error> {
        if args.len() < 2 {
            // Return help message if no command is provided
             return Err(Error::Generic(format!("{}\n\n{}",
                 "Usage: ash <command> [options]",
                 Self::format_help() // Include help format on basic usage error
             )));
        }

        let command = args[1].to_lowercase();
        let cli_args = match command.as_str() {
            "init" => CliArgs {
                command: Command::Init {
                    path: args.get(2).map(|s| s.to_owned()).unwrap_or(".".to_string()),
                },
            },
            "commit" => {
                let mut message = None; // Use Option for message initially
                let mut amend = false;
                let mut reuse_message = None;
                let mut edit = false;
                
                let mut i = 2;
                while i < args.len() {
                    match args[i].as_str() {
                        "--message" | "-m" => {
                            if i + 1 < args.len() {
                                message = Some(args[i + 1].to_owned());
                                i += 2; // Skip both flag and value
                            } else {
                                return Err(Error::Generic("--message requires a value".to_string()));
                            }
                        },
                        "--amend" => {
                            amend = true;
                            i += 1;
                        },
                        "--edit" | "-e" => {
                            edit = true;
                            i += 1;
                        },
                        "--reuse-message" | "-C" => {
                            if i + 1 < args.len() {
                                reuse_message = Some(args[i + 1].to_owned());
                                i += 2;
                            } else {
                                return Err(Error::Generic("--reuse-message requires a value".to_string()));
                            }
                        },
                        "--reedit-message" | "-c" => {
                            if i + 1 < args.len() {
                                reuse_message = Some(args[i + 1].to_owned());
                                edit = true;
                                i += 2;
                            } else {
                                return Err(Error::Generic("--reedit-message requires a value".to_string()));
                            }
                        },
                        "--file" | "-F" => {
                            if i + 1 < args.len() {
                                // Just parse the message from the file here
                                let file_path = &args[i + 1];
                                match std::fs::read_to_string(file_path) {
                                    Ok(content) => {
                                        message = Some(content);
                                        i += 2;
                                    },
                                    Err(e) => {
                                        return Err(Error::Generic(format!("Failed to read message file: {}", e)));
                                    }
                                }
                            } else {
                                return Err(Error::Generic("--file requires a value".to_string()));
                            }
                        },
                        _ => {
                            // Handle potential unknown flags or arguments
                            return Err(Error::Generic(format!("Unknown option for commit: {}", args[i])));
                        }
                    }
                }

                // No message needed with --amend (can reuse previous commit message)
                if message.is_none() && reuse_message.is_none() && !amend {
                    // Try reading from standard input or editor if no -m is provided (like git)
                    // For now, we'll require a message one way or another
                    return Err(Error::Generic("Commit message is required. Use --message/-m, --file/-F, --reuse-message/-C, or --amend".to_string()));
                }

                CliArgs {
                    command: Command::Commit {
                        message: message.unwrap_or_default(),
                        amend,
                        reuse_message,
                        edit,
                    },
                }
            },
            "add" => {
                if args.len() < 3 {
                    return Err(Error::Generic("File path(s) are required for add command".to_string()));
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
                    // Correct index check for color value
                    if arg == "--color" && i + 2 < args.len() { // Look at index i+2 relative to args start
                        Some(args[i + 2 + 1].clone()) // Argument is at i+2, value at i+3 (relative to args start)
                    } else if arg.starts_with("--color=") {
                        Some(arg.splitn(2, '=').nth(1).unwrap_or("auto").to_string())
                    } else {
                        None
                    }
                }).unwrap_or_else(|| "auto".to_string()); // Default to auto

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
                    } else if !arg.starts_with('-') { // Assume non-flag arguments are paths
                        paths.push(arg.clone());
                    } else {
                         // Handle other potential flags or return error for unknown flags
                         // return Err(Error::Generic(format!("Unknown option for diff: {}", arg)));
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
                        },
                        "-d" | "--delete" => {
                            delete = true;
                        },
                        "-f" | "--force" => {
                            force = true;
                        },
                        "-D" => {
                            delete = true;
                            force = true;
                        },
                        // Check for other potential flags if needed
                        a if a.starts_with('-') => {
                            // Allow flags to appear anywhere relative to positional args
                            // Just consume the flag
                        },
                        _ => {
                            // Treat non-flag arguments as positional: name then start_point
                            if name.is_empty() {
                                name = arg.clone();
                            } else if start_point.is_none() {
                                start_point = Some(arg.clone());
                            } else {
                                 // Too many positional arguments
                                 return Err(Error::Generic(format!("Unexpected argument for branch: {}", arg)));
                            }
                        }
                    }
                    i += 1; // Increment index for every argument processed
                }

                // If name is empty, it implies listing branches (handled by BranchCommand)
                // If delete is true, name must be provided
                if delete && name.is_empty() {
                     return Err(Error::Generic("Branch name required for delete operation".to_string()));
                }


                CliArgs {
                    command: Command::Branch {
                        name, // Can be empty for listing
                        start_point,
                        verbose,
                        delete,
                        force
                    },
                }
            },
            "checkout" => {
                if args.len() < 3 {
                    return Err(Error::Generic("No checkout target specified (branch, commit, or path)".to_string()));
                }
                 // Allow multiple targets for file checkout? Git's behavior is complex here.
                 // For now, assume one target (branch or commit).
                 // Handle `checkout -- <paths...>` separately if needed.
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
                let mut abbrev = false; // Default to false like git
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
                        },
                        "--no-abbrev-commit" => {
                            abbrev = false;
                        },
                        "--pretty" | "--format" => {
                            if i + 1 < args.len() {
                                format = args[i + 1].clone();
                                i += 1; // Increment extra for the value
                            } else {
                                 return Err(Error::Generic(format!("Option '{}' requires a value", arg)));
                            }
                        },
                        a if a.starts_with("--pretty=") || a.starts_with("--format=") => {
                             let parts: Vec<&str> = a.splitn(2, '=').collect();
                             if parts.len() == 2 {
                                format = parts[1].to_string();
                             } else {
                                 return Err(Error::Generic(format!("Invalid format for option '{}'", arg)));
                             }
                        },
                        "--oneline" => {
                            format = "oneline".to_string();
                            abbrev = true; // oneline implies abbrev
                        },
                        "-p" | "-u" | "--patch" => {
                            patch = true;
                        },
                        "-s" | "--no-patch" => {
                            patch = false;
                        },
                        "--decorate" => {
                            // Allow setting decorate without a value, default to short/auto later
                             decorate = "auto".to_string();
                        },
                        a if a.starts_with("--decorate=") => {
                             let parts: Vec<&str> = a.splitn(2, '=').collect();
                             if parts.len() == 2 {
                                decorate = parts[1].to_string();
                             } else {
                                 return Err(Error::Generic(format!("Invalid format for option '{}'", arg)));
                             }
                        },
                        "--no-decorate" => {
                            decorate = "no".to_string();
                        },
                        a if a.starts_with('-') => {
                            // Unknown flag
                             return Err(Error::Generic(format!("Unknown option for log: {}", a)));
                        },
                        _ => {
                            // This is a revision specifier
                            revisions.push(arg.clone());
                        }
                    }
                     i += 1; // Increment for the current argument
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
            "rm" => {
                // Parse rm command options
                let mut files = Vec::new();
                let mut cached = false;
                let mut force = false;
                let mut recursive = false;
                
                // Process arguments
                let mut i = 2;
                while i < args.len() {
                    let arg = &args[i];
                    match arg.as_str() {
                        "--cached" => {
                            cached = true;
                        },
                        "-f" | "--force" => {
                            force = true;
                        },
                        "-r" | "--recursive" => {
                            recursive = true;
                        },
                        a if a.starts_with('-') => {
                            // Unknown flag
                            return Err(Error::Generic(format!("Unknown option for rm: {}", a)));
                        },
                        _ => {
                            // This is a file path
                            files.push(arg.clone());
                        }
                    }
                    i += 1;
                }
                
                if files.is_empty() {
                    return Err(Error::Generic("No files specified for removal".to_string()));
                }
                
                CliArgs {
                    command: Command::Rm {
                        files,
                        cached,
                        force,
                        recursive,
                    },
                }
            },
            "merge" => {
                let mut branch = String::new();
                let mut message = None;
                let mut abort = false;
                let mut continue_merge = false;
                let mut tool = None; 

                let mut i = 2;
                while i < args.len() {
                    let arg = &args[i];
                    match arg.as_str() {
                        "--message" | "-m" => {
                            if i + 1 < args.len() {
                                message = Some(args[i + 1].clone());
                                i += 1; 
                            } else {
                                return Err(Error::Generic(format!("Option '{}' requires a value", arg)));
                            }
                        },
                        "--abort" => {
                            abort = true;
                        },
                        "--continue" => {
                            continue_merge = true;
                        },
                        "--tool" | "-t" => {  // Added tool handling
                            if i + 1 < args.len() {
                                tool = Some(args[i + 1].clone());
                                i += 1;
                            } else {
                                return Err(Error::Generic(format!("Option '{}' requires a value", arg)));
                            }
                        },
                        "--tool-only" => { 
                            tool = Some("default".to_string());
                        },
                        // Allow unknown flags for now or add error handling
                        _ if arg.starts_with('-') => {
                            return Err(Error::Generic(format!("Unknown option for merge: {}", arg)));
                        },
                        // Assume the first non-flag argument is the branch name
                        _ if branch.is_empty() => {
                            branch = arg.clone();
                        },
                        // Handle unexpected additional arguments
                        _ => {
                            return Err(Error::Generic(format!("Unexpected argument for merge: {}", arg)));
                        }
                    }
                    i += 1; // Increment index
                }

                // Special case: if --tool-only or --tool is provided without branch, it means
                // we want to just run the tool on existing conflicts
                let resolve_only = tool.is_some() && branch.is_empty() && !abort && !continue_merge;
                
                // Branch name is required unless --abort, --continue, or just running the tool
                if branch.is_empty() && !abort && !continue_merge && !resolve_only {
                    return Err(Error::Generic("No branch specified for merge and not using --abort, --continue, or --tool".to_string()));
                }
                
                // Cannot specify branch name with --abort or --continue
                if !branch.is_empty() && (abort || continue_merge) {
                    return Err(Error::Generic("Cannot specify branch name with --abort or --continue".to_string()));
                }

                CliArgs {
                    command: Command::Merge {
                        branch,
                        message,
                        abort,
                        continue_merge,
                        tool,
                    },
                }
            },
            "reset" => {
                // Parse reset options
                let mut files = Vec::new();
                let mut soft = false;
                let mut mixed = false;
                let mut hard = false;
                let mut force = false;
                let mut reuse_message = None;
                
                // Process all arguments for options
                let mut i = 2;
                while i < args.len() {
                    let arg = &args[i];
                    match arg.as_str() {
                        "--soft" => {
                            soft = true;
                            i += 1;
                        },
                        "--mixed" => {
                            mixed = true;
                            i += 1;
                        },
                        "--hard" => {
                            hard = true;
                            i += 1;
                        },
                        "--force" | "-f" => {
                            force = true;
                            i += 1;
                        },
                        "--reuse-message" | "-C" => {
                            if i + 1 < args.len() {
                                reuse_message = Some(args[i + 1].clone());
                                i += 2;
                            } else {
                                return Err(Error::Generic("--reuse-message requires a value".to_string()));
                            }
                        },
                        arg if arg.starts_with('-') => {
                            return Err(Error::Generic(format!("Unknown option for reset: {}", arg)));
                        },
                        _ => {
                            files.push(arg.clone());
                            i += 1;
                        }
                    }
                }
                
                CliArgs {
                    command: Command::Reset {
                        files,
                        soft,
                        mixed,
                        hard,
                        force,
                        reuse_message,
                    },
                }
            },
            "cherry-pick" => {
                let mut commit_args: Vec<String> = Vec::new();
                let mut continue_op = false;
                let mut abort = false;
                let mut quit = false;
                let mut mainline = None;
                
                let mut i = 2;
                while i < args.len() {
                    match args[i].as_str() {
                        "--continue" => {
                            continue_op = true;
                            i += 1;
                        },
                        "--abort" => {
                            abort = true;
                            i += 1;
                        },
                        "--quit" => {
                            quit = true;
                            i += 1;
                        },
                        "-m" | "--mainline" => {
                            if i + 1 < args.len() {
                                match args[i + 1].parse::<u32>() {
                                    Ok(val) => {
                                        mainline = Some(val);
                                        i += 2;
                                    },
                                    Err(_) => return Err(Error::Generic("--mainline requires a positive integer".to_string())),
                                }
                            } else {
                                return Err(Error::Generic("--mainline requires a value".to_string()));
                            }
                        },
                        arg if arg.starts_with('-') => {
                            return Err(Error::Generic(format!("Unknown option for cherry-pick: {}", arg)));
                        },
                        _ => {
                            commit_args.push(args[i].clone());
                            i += 1;
                        }
                    }
                }
                
                // Check for invalid combinations
                if (continue_op && abort) || (continue_op && quit) || (abort && quit) {
                    return Err(Error::Generic("Cannot combine --continue, --abort, and --quit".to_string()));
                }
                
                if (continue_op || abort || quit) && !commit_args.is_empty() {
                    return Err(Error::Generic("Cannot combine --continue, --abort, or --quit with commits".to_string()));
                }
                
                if commit_args.is_empty() && !(continue_op || abort || quit) {
                    return Err(Error::Generic("cherry-pick requires at least one commit".to_string()));
                }
                
                CliArgs {
                    command: Command::CherryPick {
                        args: commit_args,
                        r#continue: continue_op,
                        abort,
                        quit,
                        mainline,
                    },
                }
            },
            "revert" => {
                let mut commit_args: Vec<String> = Vec::new();
                let mut continue_op = false;
                let mut abort = false;
                let mut quit = false;
                let mut mainline = None;
                
                let mut i = 2;
                while i < args.len() {
                    match args[i].as_str() {
                        "--continue" => {
                            continue_op = true;
                            i += 1;
                        },
                        "--abort" => {
                            abort = true;
                            i += 1;
                        },
                        "--quit" => {
                            quit = true;
                            i += 1;
                        },
                        "-m" | "--mainline" => {
                            if i + 1 < args.len() {
                                match args[i + 1].parse::<u32>() {
                                    Ok(val) => {
                                        mainline = Some(val);
                                        i += 2;
                                    },
                                    Err(_) => return Err(Error::Generic("--mainline requires a positive integer".to_string())),
                                }
                            } else {
                                return Err(Error::Generic("--mainline requires a value".to_string()));
                            }
                        },
                        arg if arg.starts_with('-') => {
                            return Err(Error::Generic(format!("Unknown option for revert: {}", arg)));
                        },
                        _ => {
                            commit_args.push(args[i].clone());
                            i += 1;
                        }
                    }
                }
                
                // Check for invalid combinations
                if (continue_op && abort) || (continue_op && quit) || (abort && quit) {
                    return Err(Error::Generic("Cannot combine --continue, --abort, and --quit".to_string()));
                }
                
                if (continue_op || abort || quit) && !commit_args.is_empty() {
                    return Err(Error::Generic("Cannot combine --continue, --abort, or --quit with commits".to_string()));
                }
                
                if commit_args.is_empty() && !(continue_op || abort || quit) {
                    return Err(Error::Generic("revert requires at least one commit".to_string()));
                }
                
                CliArgs {
                    command: Command::Revert {
                        args: commit_args,
                        r#continue: continue_op,
                        abort,
                        quit,
                        mainline,
                    },
                }
            },
            "sprint" => {
                if args.len() < 3 {
                    // Return SprintView instead of error if no subcommand is provided
                    return Ok(CliArgs {
                        command: Command::SprintView {},
                    });
                }
                
                let subcommand = args[2].to_lowercase();
                match subcommand.as_str() {
                    "start" => {
                        if args.len() < 5 {
                            return Err(Error::Generic("sprint start requires a name and duration".to_string()));
                        }
                        
                        let name = args[3].clone();
                        let duration = match args[4].parse::<u32>() {
                            Ok(d) => d,
                            Err(_) => return Err(Error::Generic("Duration must be a positive integer".to_string())),
                        };
                        
                        CliArgs {
                            command: Command::SprintStart {
                                name,
                                duration,
                            },
                        }
                    },
                    "info" => {
                        CliArgs {
                            command: Command::SprintInfo {},
                        }
                    },
                    "commitmap" => {
                        // Optional sprint name parameter
                        let sprint_name = if args.len() > 3 {
                            Some(args[3].clone())
                        } else {
                            None
                        };
                        
                        CliArgs {
                            command: Command::SprintCommitMap {
                                sprint_name,
                            },
                        }
                    },
                    "burndown" => {
                        // Optional sprint name parameter
                        let sprint_name = if args.len() > 3 {
                            Some(args[3].clone())
                        } else {
                            None
                        };
                        
                        CliArgs {
                            command: Command::SprintBurndown {
                                sprint_name,
                            },
                        }
                    },
                    "velocity" => {
                        CliArgs {
                            command: Command::SprintVelocity {},
                        }
                    },
                    "advance" => {
                        if args.len() < 6 {
                            return Err(Error::Generic("sprint advance requires a name, start date, and end date".to_string()));
                        }
                        
                        let name = args[3].clone();
                        let start_date = args[4].clone();
                        let end_date = args[5].clone();
                        
                        CliArgs {
                            command: Command::SprintAdvance {
                                name,
                                start_date,
                                end_date,
                            },
                        }
                    },
                    _ => {
                        return Err(Error::Generic(format!("Unknown sprint subcommand: {}", subcommand)));
                    }
                }
            },
            "task" => {
                if args.len() < 3 {
                    // Dacă nu există subcomandă, rulează automat comanda "task list"
                    return Ok(CliArgs {
                        command: Command::TaskList {
                            args: Vec::new(),
                        },
                    });
                }
                
                let subcommand = args[2].to_lowercase();
                let mut args_iter = args.iter().skip(3);
                match subcommand.as_str() {
                    "create" => {
                        if args.len() < 4 {
                            return Err(Error::Generic("task create requires an id and description".to_string()));
                        }
                        
                        let id = args[3].clone();
                        let description = args[4].clone();
                        
                        // Parse optional story points
                        let story_points = if args.len() > 4 {
                            match args[4].parse::<u32>() {
                                Ok(sp) => Some(sp),
                                Err(_) => return Err(Error::Generic("Story points must be a positive integer".to_string())),
                            }
                        } else {
                            None
                        };
                        
                        CliArgs {
                            command: Command::TaskCreate {
                                id,
                                description,
                                story_points,
                            },
                        }
                    },
                    "complete" => {
                        if args.len() < 4 {
                            return Err(Error::Generic("task complete requires a task id".to_string()));
                        }
                        
                        let id = args[3].clone();
                        
                        // Check for --auto-merge flag
                        let auto_merge = args.iter().skip(4).any(|arg| arg == "--auto-merge");
                        
                        CliArgs {
                            command: Command::TaskComplete {
                                id,
                                story_points: None,
                                auto_merge,
                            },
                        }
                    },
                    "status" => {
                        if args.len() < 4 {
                            return Err(Error::Generic("Task ID is required".into()));
                        }
                        
                        let id = args[3].clone();
                        
                        return Ok(CliArgs {
                            command: Command::TaskStatus {
                                id,
                            },
                        });
                    },
                    _ => return Err(Error::Generic(format!("Unknown task subcommand: {}", subcommand))),
                }
            },
            _ => CliArgs {
                command: Command::Unknown {
                    name: command.clone(),
                },
            },
        };

        Ok(cli_args)
    }

    pub fn format_help() -> String {
        let mut help = String::new();
        
        help.push_str("Usage: ash <command> [options]\n");
        help.push_str("Commands:\n");
        help.push_str("  init [path]                       Initialize a new repository\n");
        help.push_str("  add <paths...>                    Add file contents to the index\n");
        help.push_str("  commit -m <message>               Commit changes to the repository\n");
        help.push_str("  status [--porcelain] [--color=...] Show the working tree status\n");
        help.push_str("  diff [--cached] [paths...]        Show changes (HEAD vs index or index vs workspace)\n");
        help.push_str("  branch [-v] [-d|-D] [<n> [<sp>]]  Manage branches (list, create, delete)\n");
        help.push_str("  checkout <target>                 Switch branches or restore working tree files\n");
        help.push_str("  log [--oneline] [--decorate=...]  Show commit logs\n");
        help.push_str("  rm <files...> [--cached] [-f]     Remove files from the working tree and index\n");
        help.push_str("  reset [--soft|--mixed|--hard]     Reset current HEAD to the specified state\n");
        help.push_str("  cherry-pick <commits...>          Apply changes from existing commits\n");
        help.push_str("        --continue|--abort|--quit   Control operation in progress\n");
        help.push_str("  revert <commits...>               Revert existing commits\n");
        help.push_str("        --continue|--abort|--quit   Control operation in progress\n");
        help.push_str("  merge <branch> [-m <msg>]         Merge the specified branch into the current branch\n");
        help.push_str("        --abort                     Abort the current merge resolution process\n");
        help.push_str("        --continue                  Continue the merge after resolving conflicts\n");
        help.push_str("        --tool=<tool>               Use specified tool to resolve merge conflicts\n");
        help.push_str("        --tool-only                 Run merge tool to resolve conflicts without merging\n");
        help.push_str("\n");
        help.push_str("Sprint & Task Management:\n");
        help.push_str("  sprint                         Interactive view of the current sprint\n");
        help.push_str("  sprint start <n> <duration>    Start a new sprint with specified name and duration in days\n");
        help.push_str("  sprint info                    Show information about the current sprint\n");
        help.push_str("  sprint commitmap [<sprint>]    Display a table of commits for the current or specified sprint\n");
        help.push_str("  sprint burndown [<sprint>]     Display a burndown chart for the current or specified sprint\n");
        help.push_str("  sprint velocity                Display a velocity chart showing progress across sprints\n");
        help.push_str("  sprint advance <n> <s> <e>     Advance to a new sprint with name, start and end dates\n");
        help.push_str("  task                           Display and manage all tasks (interactive UI)\n");
        help.push_str("  task create <id> <desc> [<sp>]    Create a new task in the current sprint\n");
        help.push_str("  task complete <id> [--auto-merge] Mark a task as completed\n");
        help.push_str("  task status <id>                  Show status information for a task\n");
        help.push_str("\n");
        help.push_str("Common Options:\n");
        help.push_str("  (Options specific to commands listed above)\n");
        help.push_str("  --help                           Display this help message\n");
        
        help
    }
}