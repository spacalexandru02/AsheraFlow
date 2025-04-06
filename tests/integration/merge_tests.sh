#!/bin/bash
# Test suite for the ASH merge command
# This script tests fast-forward, recursive merge, and various conflict scenarios.

# --- Configuration ---
# Find the ASH executable
if [ -n "$1" ]; then
    ASH_EXECUTABLE="$1"
elif [ -f "./target/release/AsheraFlow" ]; then
    ASH_EXECUTABLE="$(pwd)/target/release/AsheraFlow" # Assumes script run from project root
elif [ -f "./target/debug/AsheraFlow" ]; then
    ASH_EXECUTABLE="$(pwd)/target/debug/AsheraFlow" # Assumes script run from project root
elif [ -f "../../target/release/AsheraFlow" ]; then
     # If run from tests/integration directory
    ASH_EXECUTABLE="$(pwd)/../../target/release/AsheraFlow"
elif [ -f "../../target/debug/AsheraFlow" ]; then
     # If run from tests/integration directory
    ASH_EXECUTABLE="$(pwd)/../../target/debug/AsheraFlow"
else
    echo "ASH executable not found. Build the project or provide the path as an argument."
    echo "Usage: $0 [path-to-ash-executable] (run from project root or tests/integration)"
    exit 1
fi

# Ensure ASH_EXECUTABLE is an absolute path
ASH_EXECUTABLE=$(cd "$(dirname "$ASH_EXECUTABLE")" && pwd)/$(basename "$ASH_EXECUTABLE")

# --- Logging Setup ---
# Get the directory where the script resides
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
# Define the log file path within the script's directory
LOG_FILE="$SCRIPT_DIR/merge_tests.log"
# Clear the log file at the start of the run
> "$LOG_FILE"
# --- End Logging Setup ---

# Log initial messages to file and console
echo "Using ASH executable: $ASH_EXECUTABLE" | tee -a "$LOG_FILE"
ASH_CMD="$ASH_EXECUTABLE" # Alias for easier use

set -e # Exit immediately if a command exits with a non-zero status.
# set -x # Uncomment for detailed command execution debugging

# --- Test Environment Setup ---
TEST_DIR=$(mktemp -d)
echo "Using temporary directory for test repos: ${TEST_DIR}" | tee -a "$LOG_FILE"
echo "Logging detailed output to: ${LOG_FILE}" | tee -a "$LOG_FILE"

ORIGINAL_PWD=$(pwd) # Save original PWD if needed, although TEST_DIR is used
cd "$TEST_DIR" || exit 1


# --- Colors and Counters ---
RED="\033[0;31m"
GREEN="\033[0;32m"
YELLOW="\033[0;33m"
BLUE="\033[0;34m"
RESET="\033[0m"
TESTS_PASSED=0
TESTS_FAILED=0

# --- Helper Functions ---
function setup_repo() {
    local repo_name=${1:-"test_repo"}
    rm -rf "$repo_name" .ash 2>/dev/null || true
    mkdir -p "$repo_name"
    cd "$repo_name"
    # Redirect init output to log file
    "$ASH_CMD" init . >> "$LOG_FILE" 2>&1
    # Configure git user locally for commits (important for Author info)
    export GIT_AUTHOR_NAME="Test User"
    export GIT_AUTHOR_EMAIL="test@example.com"
    # Log initialization message
    echo -e "${BLUE}Initialized repo in $(pwd)${RESET}" | tee -a "$LOG_FILE"
    cd .. # Go back to TEST_DIR
}

function create_commit() {
    local repo_name="$1"
    local file_name="$2"
    local content="$3"
    local message="$4"
    local branch
    # Ensure we are checking the correct HEAD file location relative to the repo
    branch=$(cd "$repo_name" && cat .ash/HEAD 2>/dev/null | sed 's|ref: refs/heads/||' || echo "master")

    echo "$content" > "$repo_name/$file_name"
    # Redirect add/commit output to log file
    (cd "$repo_name" && "$ASH_CMD" add "$file_name" >> "$LOG_FILE" 2>&1) || { echo "Add command failed in create_commit" | tee -a "$LOG_FILE"; exit 1; }
    (cd "$repo_name" && "$ASH_CMD" commit -m "$message" >> "$LOG_FILE" 2>&1) || { echo "Commit command failed in create_commit" | tee -a "$LOG_FILE"; exit 1; }
    # Log commit message
    echo "  Commit on '$branch': '$message' ($file_name)" | tee -a "$LOG_FILE"
}


function run_cmd() {
    local repo_name="$1"
    shift # Remove repo_name from args
    # Log command being run
    echo -e "${YELLOW}  CMD [in $repo_name]: ${ASH_CMD} $@${RESET}" | tee -a "$LOG_FILE"
    # Redirect stdout and stderr to the main log file, appending
    if (cd "$repo_name" && "$ASH_CMD" "$@") >> "$LOG_FILE" 2>&1; then
        # Log success
        echo -e "${GREEN}  CMD OK${RESET}" | tee -a "$LOG_FILE"
        return 0
    else
        local exit_code=$?
        # Log failure
        echo -e "${RED}  CMD FAILED (Exit Code: $exit_code)${RESET}" | tee -a "$LOG_FILE"
        # Show relevant part of log on console in case of failure
        echo -e "${RED}--- Start Log Output Snippet [${LOG_FILE}] ---${RESET}" # Log snippet header to console only
        tail -n 50 "$LOG_FILE" # Show last N lines on console
        echo -e "${RED}--- End Log Output Snippet (Full log in ${LOG_FILE}) ---${RESET}" # Log snippet footer to console only
        return $exit_code
    fi
}


# Function to check command success (ignores output)
function check_success() {
    local repo_name="$1"
    shift
    echo -e "${YELLOW}  CHECK SUCCESS [in $repo_name]: ${ASH_CMD} $@${RESET}" | tee -a "$LOG_FILE"
    if (cd "$repo_name" && "$ASH_CMD" "$@") >> "$LOG_FILE" 2>&1; then
        echo -e "${GREEN}  CHECK OK${RESET}" | tee -a "$LOG_FILE"
        return 0
    else
        echo -e "${RED}  CHECK FAILED${RESET}" | tee -a "$LOG_FILE"
        return 1
    fi
}

# Function to check command failure (ignores output)
function check_failure() {
    local repo_name="$1"
    shift
    echo -e "${YELLOW}  CHECK FAILURE [in $repo_name]: ${ASH_CMD} $@${RESET}" | tee -a "$LOG_FILE"
    if ! (cd "$repo_name" && "$ASH_CMD" "$@") >> "$LOG_FILE" 2>&1; then
        echo -e "${GREEN}  CHECK OK (Expected Failure)${RESET}" | tee -a "$LOG_FILE"
        return 0
    else
        echo -e "${RED}  CHECK FAILED (Expected Failure, but Succeeded)${RESET}" | tee -a "$LOG_FILE"
        return 1
    fi
}

function assert_file_contains() {
    local repo_name="$1"
    local file_path="$2"
    local expected_content="$3"
    local msg="$4"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    # Construct full path based on whether repo_name is provided
    local full_file_path
    if [ -z "$repo_name" ]; then
        # If repo_name is empty, assume file_path is relative to TEST_DIR
        full_file_path="$file_path"
    else
        full_file_path="$repo_name/$file_path" # Path is inside the repo directory
    fi

    # Check if file exists before trying to grep
    if [ ! -f "$full_file_path" ]; then
        echo -e "${RED}FAIL: $msg - File '$full_file_path' does not exist for checking content.${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi

    if grep -qF "$expected_content" "$full_file_path"; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - File '$full_file_path' does not contain '$expected_content'. Actual content:${RESET}" | tee -a "$LOG_FILE"
        # Log file content to the log file as well
        cat "$full_file_path" >> "$LOG_FILE"
        cat "$full_file_path" # Also show on console
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}


function assert_file_exists() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
     echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ -f "$repo_name/$file_path" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' does not exist.${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_file_not_exists() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
     echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ ! -f "$repo_name/$file_path" ] && [ ! -d "$repo_name/$file_path" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Path '$repo_name/$file_path' exists when it shouldn't.${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_conflict_markers() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    # Check if file exists before trying to grep
    if [ ! -f "$repo_name/$file_path" ]; then
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' does not exist for checking conflict markers.${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi

    if grep -q '<<<<<<<' "$repo_name/$file_path" && grep -q '=======' "$repo_name/$file_path" && grep -q '>>>>>>>' "$repo_name/$file_path"; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Conflict markers not found in '$repo_name/$file_path'. Actual content:${RESET}" | tee -a "$LOG_FILE"
        # Log file content to the log file as well
        cat "$repo_name/$file_path" >> "$LOG_FILE"
        cat "$repo_name/$file_path" # Also show on console
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}


function get_head_oid() {
    local repo_name="$1"
    cat "$repo_name/.ash/refs/heads/master" 2>/dev/null || cat "$repo_name/.ash/HEAD" 2>/dev/null || echo "unknown_oid"
}

function get_branch_oid() {
    local repo_name="$1"
    local branch_name="$2"
    cat "$repo_name/.ash/refs/heads/$branch_name" 2>/dev/null || echo "unknown_oid"
}

# --- Test Cases ---

function test_fast_forward_merge() {
    echo -e "\n${BLUE}--- Test: Fast-Forward Merge ---${RESET}" | tee -a "$LOG_FILE"
    local repo="ff_repo"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Initial content" "Initial commit"
    local initial_commit_oid=$(get_head_oid "$repo")

    run_cmd "$repo" branch feature
    run_cmd "$repo" checkout feature
    create_commit "$repo" "file2.txt" "Feature content" "Add feature file"
    local feature_commit_oid=$(get_branch_oid "$repo" "feature")

    run_cmd "$repo" checkout master
    run_cmd "$repo" merge feature
    local master_commit_oid=$(get_head_oid "$repo")

    assert_file_exists "$repo" "file1.txt" "FF Merge: file1.txt should exist"
    assert_file_exists "$repo" "file2.txt" "FF Merge: file2.txt should exist"
    assert_file_contains "$repo" "file2.txt" "Feature content" "FF Merge: file2.txt content check"

    if [ "$master_commit_oid" == "$feature_commit_oid" ]; then
        echo -e "${GREEN}PASS: FF Merge: master OID matches feature OID${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: FF Merge: master OID ($master_commit_oid) does not match feature OID ($feature_commit_oid)${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    cd "$TEST_DIR" # Ensure we are in the base test directory
}

function test_already_up_to_date() {
    echo -e "\n${BLUE}--- Test: Already Up-to-Date Merge ---${RESET}" | tee -a "$LOG_FILE"
    local repo="uptodate_repo"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Content" "Initial"
    run_cmd "$repo" branch feature
    # Merge feature into master (should be fast-forward)
    run_cmd "$repo" merge feature # First merge output goes to log file
    # Add a small pause to allow the OS to release the lock file
    sleep 0.1
    # Try merging again, capture output specifically for assertion
    local merge_output
    merge_output=$(cd "$repo" && "$ASH_CMD" merge feature 2>&1) || true # Capture stdout/stderr, ignore exit code for check
    echo "$merge_output" >> "$LOG_FILE" # Append this specific output to main log too
    # Assert directly on the captured output string
    echo -e "${YELLOW}TEST: Already Up-to-Date: Output check${RESET}" | tee -a "$LOG_FILE"
    if echo "$merge_output" | grep -qF "Already up to date."; then
         echo -e "${GREEN}PASS: Already Up-to-Date: Output check${RESET}" | tee -a "$LOG_FILE"
         TESTS_PASSED=$((TESTS_PASSED + 1))
    else
         echo -e "${RED}FAIL: Already Up-to-Date: Output check - Expected 'Already up to date.' not found. Actual output:${RESET}" | tee -a "$LOG_FILE"
         echo "$merge_output" # Show output on console on failure
         TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    cd "$TEST_DIR"
}


function test_recursive_merge_no_conflict() {
    echo -e "\n${BLUE}--- Test: Recursive Merge (No Conflict) ---${RESET}" | tee -a "$LOG_FILE"
    local repo="recursive_repo"
    setup_repo "$repo"
    create_commit "$repo" "common.txt" "Base" "Base commit"
    local base_oid=$(get_head_oid "$repo")

    # Master branch changes
    create_commit "$repo" "master_file.txt" "Master change" "Commit on master"
    local master_oid=$(get_head_oid "$repo")

    # Feature branch changes
    # Replace 'checkout -b' with separate 'branch' and 'checkout' commands
    run_cmd "$repo" branch feature "$base_oid"     # Create feature branch pointing to base_oid
    run_cmd "$repo" checkout feature               # Switch to the new feature branch
    create_commit "$repo" "feature_file.txt" "Feature change" "Commit on feature"
    local feature_oid=$(get_branch_oid "$repo" "feature")

    # Merge
    run_cmd "$repo" checkout master
    run_cmd "$repo" merge feature
    local merge_commit_oid=$(get_head_oid "$repo")

    assert_file_exists "$repo" "common.txt" "Recursive Merge: common.txt exists"
    assert_file_exists "$repo" "master_file.txt" "Recursive Merge: master_file.txt exists"
    assert_file_exists "$repo" "feature_file.txt" "Recursive Merge: feature_file.txt exists"
    assert_file_contains "$repo" "master_file.txt" "Master change" "Recursive Merge: master_file content"
    assert_file_contains "$repo" "feature_file.txt" "Feature change" "Recursive Merge: feature_file content"

    if [ "$merge_commit_oid" != "$master_oid" ] && [ "$merge_commit_oid" != "$feature_oid" ]; then
         echo -e "${GREEN}PASS: Recursive Merge: New merge commit created ($merge_commit_oid)${RESET}" | tee -a "$LOG_FILE"
         TESTS_PASSED=$((TESTS_PASSED + 1))
         # Optionally check merge commit parents using log
    else
         echo -e "${RED}FAIL: Recursive Merge: Did not create a new merge commit.${RESET}" | tee -a "$LOG_FILE"
         TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    cd "$TEST_DIR"
}

function test_content_conflict() {
    echo -e "\n${BLUE}--- Test: Content Conflict Merge ---${RESET}" | tee -a "$LOG_FILE"
    local repo="conflict_repo"
    setup_repo "$repo"
    create_commit "$repo" "conflict.txt" "line1\nline2\nline3" "Base content"
    local base_oid=$(get_head_oid "$repo")

    # Master changes
    echo -e "line1_master\nline2\nline3" > "$repo/conflict.txt"
    run_cmd "$repo" add conflict.txt
    run_cmd "$repo" commit -m "Modify line 1 on master"

    # Feature changes
    # Replace 'checkout -b' with separate 'branch' and 'checkout' commands
    run_cmd "$repo" branch feature "$base_oid"
    run_cmd "$repo" checkout feature
    echo -e "line1\nline2\nline3_feature" > "$repo/conflict.txt"
    run_cmd "$repo" add conflict.txt
    run_cmd "$repo" commit -m "Modify line 3 on feature"

    # Merge and expect failure
    run_cmd "$repo" checkout master
    echo -e "${YELLOW}TEST: Content Conflict: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
    if ! run_cmd "$repo" merge feature; then
        echo -e "${GREEN}PASS: Content Conflict: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: Content Conflict: Merge command succeeded unexpectedly${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi

    assert_conflict_markers "$repo" "conflict.txt" "Content Conflict: Markers check"
    # Check index status for conflict (this requires status to show conflicts)
    # (cd "$repo" && "$ASH_CMD" status) # Manual check for now
    cd "$TEST_DIR"
}

function test_file_directory_conflict() {
    echo -e "\n${BLUE}--- Test: File/Directory Conflict Merge ---${RESET}" | tee -a "$LOG_FILE"
    local repo="filedir_repo"
    setup_repo "$repo"
    create_commit "$repo" "dummy.txt" "dummy" "Initial"
    local base_oid=$(get_head_oid "$repo")

    # Master: Create file a/b
    mkdir "$repo/a"
    echo "master file" > "$repo/a/b"
    run_cmd "$repo" add a/b
    run_cmd "$repo" commit -m "Add file a/b on master"

    # Feature: Create file a
    # Replace 'checkout -b' with separate 'branch' and 'checkout' commands
    run_cmd "$repo" branch feature "$base_oid"
    run_cmd "$repo" checkout feature
    echo "feature file" > "$repo/a"
    run_cmd "$repo" add a
    run_cmd "$repo" commit -m "Add file a on feature"

    # Merge and expect failure
    run_cmd "$repo" checkout master
    echo -e "${YELLOW}TEST: File/Dir Conflict: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
    if ! run_cmd "$repo" merge feature; then
        echo -e "${GREEN}PASS: File/Dir Conflict: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: File/Dir Conflict: Merge command succeeded unexpectedly${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    # Check for specific error message or conflicted state in index/status if implemented
    cd "$TEST_DIR"
}

function test_modify_delete_conflict() {
    echo -e "\n${BLUE}--- Test: Modify/Delete Conflict Merge ---${RESET}" | tee -a "$LOG_FILE"
    local repo="moddel_repo"
    setup_repo "$repo"
    create_commit "$repo" "moddel.txt" "Original" "Base commit"
    local base_oid=$(get_head_oid "$repo")

    # Master: Modify file
    echo "Modified on master" > "$repo/moddel.txt"
    run_cmd "$repo" add moddel.txt
    run_cmd "$repo" commit -m "Modify on master"

    # Feature: Delete file
    # Replace 'checkout -b' with separate 'branch' and 'checkout' commands
    run_cmd "$repo" branch feature "$base_oid"
    run_cmd "$repo" checkout feature
    rm "$repo/moddel.txt"
    run_cmd "$repo" add moddel.txt # Use add to record deletion
    run_cmd "$repo" commit -m "Delete on feature"

    # Merge and expect failure
    run_cmd "$repo" checkout master
    echo -e "${YELLOW}TEST: Modify/Delete Conflict: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
    if ! run_cmd "$repo" merge feature; then
        echo -e "${GREEN}PASS: Modify/Delete Conflict: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: Modify/Delete Conflict: Merge command succeeded unexpectedly${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    # Check for specific error message or conflicted state in index/status if implemented
    cd "$TEST_DIR"
}

function test_merge_fail_untracked_overwrite() {
    echo -e "\n${BLUE}--- Test: Merge Fail (Untracked Overwrite) ---${RESET}" | tee -a "$LOG_FILE"
    local repo="untracked_repo"
    setup_repo "$repo"
    create_commit "$repo" "common.txt" "Base" "Base commit"
    local base_oid=$(get_head_oid "$repo")
    create_commit "$repo" "master_file.txt" "Master" "Master change"
    # Replace 'checkout -b' with separate 'branch' and 'checkout' commands
    run_cmd "$repo" branch feature "$base_oid"
    run_cmd "$repo" checkout feature
    create_commit "$repo" "feature_file.txt" "Feature" "Feature change" # This file will conflict

    # Create untracked file that merge would create
    run_cmd "$repo" checkout master
    echo "Untracked content" > "$repo/feature_file.txt"

    # Merge and expect failure
    echo -e "${YELLOW}TEST: Untracked Overwrite: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
    if ! run_cmd "$repo" merge feature; then
        echo -e "${GREEN}PASS: Untracked Overwrite: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: Untracked Overwrite: Merge command succeeded unexpectedly${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    assert_file_contains "$repo" "feature_file.txt" "Untracked content" "Untracked Overwrite: File should remain unchanged"
    cd "$TEST_DIR"
}

function test_merge_fail_uncommitted_changes() {
    echo -e "\n${BLUE}--- Test: Merge Fail (Uncommitted Changes) ---${RESET}" | tee -a "$LOG_FILE"
    local repo="uncommitted_repo"
    setup_repo "$repo"
    create_commit "$repo" "common.txt" "Base" "Base commit"
    local base_oid=$(get_head_oid "$repo")
    create_commit "$repo" "master_file.txt" "Master" "Master change"
    # Replace 'checkout -b' with separate 'branch' and 'checkout' commands
    run_cmd "$repo" branch feature "$base_oid"
    run_cmd "$repo" checkout feature
    create_commit "$repo" "feature_file.txt" "Feature" "Feature change"

    # Modify tracked file without committing
    run_cmd "$repo" checkout master
    echo "Uncommitted modification" >> "$repo/master_file.txt"

    # Merge and expect failure
    echo -e "${YELLOW}TEST: Uncommitted Changes: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
    if ! run_cmd "$repo" merge feature; then
        echo -e "${GREEN}PASS: Uncommitted Changes: Merge command failed as expected${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: Uncommitted Changes: Merge command succeeded unexpectedly${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    assert_file_contains "$repo" "master_file.txt" "Uncommitted modification" "Uncommitted Changes: File should retain changes"
    cd "$TEST_DIR"
}


# --- Run Tests ---

test_fast_forward_merge
test_already_up_to_date
test_recursive_merge_no_conflict
test_content_conflict
test_file_directory_conflict
test_modify_delete_conflict
test_merge_fail_untracked_overwrite
test_merge_fail_uncommitted_changes

# --- Summary ---
echo -e "\n${BLUE}--- Test Summary ---${RESET}" | tee -a "$LOG_FILE"
echo -e "${GREEN}Tests Passed: $TESTS_PASSED${RESET}" | tee -a "$LOG_FILE"
if [ "$TESTS_FAILED" -gt 0 ]; then
    echo -e "${RED}Tests Failed: $TESTS_FAILED${RESET}" | tee -a "$LOG_FILE"
else
    echo -e "${GREEN}Tests Failed: $TESTS_FAILED${RESET}" | tee -a "$LOG_FILE"
fi

# --- Cleanup ---
cd "$ORIGINAL_PWD" # Go back to original directory before removing TEST_DIR
rm -rf "$TEST_DIR"
echo "Cleaned up temporary directory: $TEST_DIR" | tee -a "$LOG_FILE"
# Log file remains in tests/integration/merge_tests.log

# Exit with status code indicating failure if any tests failed
if [ "$TESTS_FAILED" -gt 0 ]; then
    exit 1
else
    exit 0
fi