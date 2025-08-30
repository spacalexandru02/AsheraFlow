AsheraFlow 🚀
AsheraFlow is an innovative Version Control System (VCS) built entirely in Rust, specially adapted for Agile teams using the Scrum methodology. It seamlessly integrates Scrum artifacts, such as sprints and tasks, directly into the command-line interface (CLI), aiming to eliminate the gap between code management and project management.

This project was developed by Spac Alexandru Raul as a bachelor's thesis at Babeș-Bolyai University, under the guidance of conf. dr. SUCIU Dan Mircea.

✨ Key Features
AsheraFlow combines the power of a Git-inspired, content-addressable storage system with features designed to simplify the Scrum workflow.

Standard VCS Functionality
Repository Management: init, status

Core Workflow: add, commit, rm, reset

Branching & Merging: branch, checkout, merge with a 3-way merge strategy

History & Inspection: log, diff (using the Myers algorithm)

Advanced Operations: cherry-pick, revert for granular commit manipulation

Native Scrum Integration
Sprint Management: Start, advance, and get information about sprints directly from the CLI (sprint start, sprint info).

Task Management: Create, list, and complete tasks (task create, task list, task complete).

Creating a task automatically generates a dedicated branch (e.g., sprint-1-task-42).

Commits made on a task branch are automatically associated with that task.

Completing a task can automatically merge its branch into the sprint's branch.

📊 TUI (Text-based User Interface) Visualizations: Generate key Scrum reports right in your terminal:

sprint burndown: Track sprint progress against an ideal line.

sprint velocity: Visualize the team's performance across the latest sprints.

sprint commitmap: View a map of commit activity during a sprint.

🛠️ Installation and Setup
AsheraFlow is built using the standard Rust toolchain.

Prerequisites:

Rust and Cargo (Install from rust-lang.org).

Steps:

Clone the repository:

git clone ...
cd AsheraFlow

Build the project:

cargo build --release

The optimized executable will be located in target/release/ash.

(Recommended) Add to PATH:
To be able to run the ash command from any directory, move the executable to a location in your PATH environment variable.

# For Linux/macOS
sudo mv target/release/ash /usr/local/bin/

⚙️ Usage
Getting Started: First Repository
To start using AsheraFlow in a project directory, initialize a new repository:

# Navigate to the project directory
cd my-scrum-project

# Initialize AsheraFlow
ash init

This command creates a hidden .ash/ directory, which will store all the versioning and Scrum metadata.

Standard VCS Workflow
The daily workflow for version control is similar to Git's.

Check the project status:

ash status

Add files to the staging area:

ash add src/main.rs README.md

Commit your changes:

ash commit -m "Implement initial login functionality"

View the project history:

ash log --oneline --decorate

Create and switch between branches:

# Create a new branch
ash branch feature/user-profile

# Switch to the new branch
ash checkout feature/user-profile

Integrated Scrum Workflow
This is where AsheraFlow shines. Manage your sprints and tasks without leaving the terminal.

Start a new sprint:
This command creates a new sprint-authentication branch and switches to it.

ash sprint start authentication 14
# Usage: ash sprint start <sprint-name> <duration-in-days>

Create a task for the sprint:
This automatically creates and checks out a new branch, for example sprint-authentication-task-AUTH-101.

ash task create AUTH-101 "Add password hashing" 5
# Usage: ash task create <task-id> "<description>" [story-points]

Work on the task:
Now, on the task branch, you can make changes to the code and commit them as usual. The commits will be linked to the AUTH-101 task.

# ...write code...
ash add src/auth.rs
ash commit -m "Feat: Implement bcrypt hashing for passwords"

Complete the task:
Once finished, this command marks the task as "Done" and automatically merges the task branch back into the sprint's branch (sprint-authentication).

ash task complete AUTH-101

Monitor Sprint Progress:
At any time, you can generate visual reports.

# See if you are on the right track
ash sprint burndown

# Check the team's historical performance
ash sprint velocity

🏗️ Architecture
The system is designed with a clear modular architecture to separate responsibilities:

CLI Layer (cli): The entry point that parses user commands and arguments.

Commands Layer (commands): Contains the specific logic for each command (e.g., init.rs, commit.rs, sprint.rs).

Core Layer (core): The heart of the application. It implements the fundamental VCS logic, including:

database: Manages the storage of objects (blobs, trees, commits).

index: Manages the staging area.

refs: Manages references like branches and HEAD.

Scrum Metadata Modules: Manages the serialization and storage of data about sprints and tasks in the VCS database.

🗺️ Future Plans
AsheraFlow is currently a functional prototype. The next steps are focused on expanding its capabilities for team collaboration in real-world scenarios.

🌐 Network Functionality: The main priority is to implement distributed capabilities, including the clone, fetch, push, and pull commands, to allow for team collaboration.

🏢 Workspace Virtualization: For corporate environments with massive monorepos, an innovative direction would be to implement a GVFS-style virtual file system. This would allow for an initial clone of just the metadata, with file content being downloaded on demand, drastically reducing checkout times and local storage space required.

Advanced Integrations: Deeper integration with other developer tools and CI/CD pipelines.

📜 License
This project is licensed under the MIT License. See the LICENSE file for details.
