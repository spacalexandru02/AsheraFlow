#!/bin/bash
# Test suite for the ASH reset command
# This script tests --soft, --mixed (default), --hard resets, and path resets.

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
LOG_FILE="$SCRIPT_DIR/reset_tests.log"
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

ORIGINAL_PWD=$(pwd) # Save original PWD
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

# Simplified OID getter assuming log outputs OID first on a line
function get_oid() {
    local repo_name="$1"
    (cd "$repo_name" && "$ASH_CMD" log --oneline -n 1 | head -n 1 | cut -d ' ' -f 1) 2>/dev/null || echo "unknown_oid"
}

# Get OID of a specific file from the index (using status --porcelain)
# NOTE: This relies on the status command supporting --porcelain and showing OIDs.
# Adjust if your status command output differs.
function get_file_oid_from_index() {
    local repo_name="$1"
    local file_path="$2"
    # Example parsing porcelain output like: " M stage_oid work_oid path" or "A  stage_oid work_oid path"
    # This might need adjustment based on your actual `ash status --porcelain` output format.
    (cd "$repo_name" && "$ASH_CMD" status --porcelain | grep " $file_path$" | awk '{print $3}') 2>/dev/null || echo "not_in_index"
}


# Check if a file exists
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

# Check if a file does NOT exist
function assert_file_not_exists() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ ! -f "$repo_name/$file_path" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' exists when it shouldn't.${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Check if a file contains specific content
function assert_file_contains() {
    local repo_name="$1"
    local file_path="$2"
    local expected_content="$3"
    local msg="$4"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ ! -f "$repo_name/$file_path" ]; then
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' does not exist to check content.${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
    if grep -qF "$expected_content" "$repo_name/$file_path"; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' does not contain '$expected_content'. Actual content:${RESET}" | tee -a "$LOG_FILE"
        cat "$repo_name/$file_path" >> "$LOG_FILE" # Log actual content
        cat "$repo_name/$file_path" # Show actual content on console
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Check if HEAD points to the expected commit
function assert_head_is() {
    local repo_name="$1"
    local expected_oid="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    local head_ref_content
    head_ref_content=$(cat "$repo_name/.ash/HEAD" 2>/dev/null)
    local actual_oid=""

    if [[ "$head_ref_content" == ref:* ]]; then
        # Symbolic ref, resolve it
        local ref_path=${head_ref_content#ref: }
        ref_path=$(echo "$ref_path" | xargs) # Trim whitespace
        actual_oid=$(cat "$repo_name/.ash/$ref_path" 2>/dev/null)
    else
        # Direct OID (detached HEAD)
        actual_oid="$head_ref_content"
    fi

    if [ "$actual_oid" == "$expected_oid" ]; then
        echo -e "${GREEN}PASS: $msg (HEAD is $actual_oid)${RESET}" | tee -a "$LOG_FILE"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Expected HEAD OID '$expected_oid', but got '$actual_oid'. HEAD content: '$head_ref_content' ${RESET}" | tee -a "$LOG_FILE"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Check if `ash diff --cached` shows any output (indicating index differs from HEAD)
function assert_index_differs_from_head() {
    local repo_name="$1"
    local should_differ="$2" # "true" or "false"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    local diff_output
    diff_output=$( (cd "$repo_name" && "$ASH_CMD" diff --cached) 2>&1 )

    if [ "$should_differ" == "true" ]; then
        # --- CORECTAT AICI ---
        if [ -n "$diff_output" ]; then # Pass if diff is NOT empty
            echo -e "${GREEN}PASS: $msg (Index differs from HEAD as expected)${RESET}" | tee -a "$LOG_FILE"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${RED}FAIL: $msg - Expected index to differ from HEAD, but 'diff --cached' was empty.${RESET}" | tee -a "$LOG_FILE"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    else # should_differ == "false"
        # --- CORECTAT AICI (deși logica era probabil ok, verificăm consistența) ---
        if [ -z "$diff_output" ]; then # Pass if diff IS empty
            echo -e "${GREEN}PASS: $msg (Index matches HEAD as expected)${RESET}" | tee -a "$LOG_FILE"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${RED}FAIL: $msg - Expected index to match HEAD, but 'diff --cached' showed changes:${RESET}" | tee -a "$LOG_FILE"
            echo "$diff_output" >> "$LOG_FILE" # Log the diff
            echo "$diff_output" # Show diff on console
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    fi
}

# --- Test Cases ---

# Setup function for standard reset tests
function setup_standard_repo() {
    local repo_name="$1"
    setup_repo "$repo_name"
    create_commit "$repo_name" "file1.txt" "Content C1" "Commit C1"
    C1_OID=$(get_oid "$repo_name")
    create_commit "$repo_name" "file2.txt" "Content C2" "Commit C2"
    C2_OID=$(get_oid "$repo_name")
    # Modify file1 and add file3 for C3
    echo "Modified file1 C3" > "$repo_name/file1.txt"
    echo "New file3 C3" > "$repo_name/file3.txt"
    run_cmd "$repo_name" add file1.txt file3.txt
    # --- CORECȚIA ESTE AICI ---
    run_cmd "$repo_name" commit -m "Commit C3" # Direct commit call after add
    C3_OID=$(get_oid "$repo_name")

    # Unstaged change after C3
    echo "Unstaged change file1" >> "$repo_name/file1.txt"
    echo "Untracked file4" > "$repo_name/file4.txt"

    # Export OIDs for tests to use
    export C1_OID C2_OID C3_OID
}

function test_soft_reset() {
    echo -e "\n${BLUE}--- Test: Soft Reset ---${RESET}" | tee -a "$LOG_FILE"
    local repo="soft_reset_repo"
    setup_standard_repo "$repo"

    run_cmd "$repo" reset --soft "$C1_OID"

    assert_head_is "$repo" "$C1_OID" "Soft Reset: HEAD should be C1"
    assert_index_differs_from_head "$repo" "true" "Soft Reset: Index should differ from new HEAD (C1)"
    # Verify index still reflects C3 state by checking a file OID (conceptual, replace if possible)
    # local file1_index_oid=$(get_file_oid_from_index "$repo" "file1.txt")
    # echo "DEBUG: file1 OID in index after soft reset: $file1_index_oid" >> "$LOG_FILE"
    # Add assertion here if get_file_oid_from_index is reliable

    assert_file_exists "$repo" "file1.txt" "Soft Reset: Working dir file1.txt should exist"
    assert_file_exists "$repo" "file2.txt" "Soft Reset: Working dir file2.txt should exist"
    assert_file_exists "$repo" "file3.txt" "Soft Reset: Working dir file3.txt should exist (from C3)"
    assert_file_exists "$repo" "file4.txt" "Soft Reset: Working dir file4.txt should exist (untracked)"
    assert_file_contains "$repo" "file1.txt" "Unstaged change file1" "Soft Reset: Working dir file1.txt should contain unstaged changes"
    cd "$TEST_DIR"
}

function test_mixed_reset() {
    echo -e "\n${BLUE}--- Test: Mixed Reset (Default) ---${RESET}" | tee -a "$LOG_FILE"
    local repo="mixed_reset_repo"
    setup_standard_repo "$repo"

    run_cmd "$repo" reset "$C1_OID" # No mode specified, defaults to mixed

    assert_head_is "$repo" "$C1_OID" "Mixed Reset: HEAD should be C1"
    assert_index_differs_from_head "$repo" "false" "Mixed Reset: Index should match new HEAD (C1)"
    # Verify index reflects C1 state by checking 'diff --cached' is empty above

    assert_file_exists "$repo" "file1.txt" "Mixed Reset: Working dir file1.txt should exist"
    assert_file_exists "$repo" "file2.txt" "Mixed Reset: Working dir file2.txt should exist"
    assert_file_exists "$repo" "file3.txt" "Mixed Reset: Working dir file3.txt should exist (from C3, now unstaged)"
    assert_file_exists "$repo" "file4.txt" "Mixed Reset: Working dir file4.txt should exist (untracked)"
    assert_file_contains "$repo" "file1.txt" "Unstaged change file1" "Mixed Reset: Working dir file1.txt should still contain unstaged changes"
    assert_file_contains "$repo" "file3.txt" "New file3 C3" "Mixed Reset: Working dir file3.txt should still contain C3 content (now unstaged)"
    cd "$TEST_DIR"
}

function test_hard_reset() {
    echo -e "\n${BLUE}--- Test: Hard Reset ---${RESET}" | tee -a "$LOG_FILE"
    local repo="hard_reset_repo"
    setup_standard_repo "$repo"

    run_cmd "$repo" reset --hard "$C1_OID"

    assert_head_is "$repo" "$C1_OID" "Hard Reset: HEAD should be C1"
    assert_index_differs_from_head "$repo" "false" "Hard Reset: Index should match new HEAD (C1)"

    assert_file_exists "$repo" "file1.txt" "Hard Reset: Working dir file1.txt should exist"
    assert_file_not_exists "$repo" "file2.txt" "Hard Reset: Working dir file2.txt should NOT exist (added in C2)"
    assert_file_not_exists "$repo" "file3.txt" "Hard Reset: Working dir file3.txt should NOT exist (added in C3)"
    assert_file_exists "$repo" "file4.txt" "Hard Reset: Working dir file4.txt should still exist (untracked)" # Hard reset doesn't touch untracked files
    assert_file_contains "$repo" "file1.txt" "Content C1" "Hard Reset: Working dir file1.txt should contain C1 content"
    assert_file_contains "$repo" "file4.txt" "Untracked file4" "Hard Reset: Untracked file4 content should be preserved"
    cd "$TEST_DIR"
}

function test_path_reset() {
    echo -e "\n${BLUE}--- Test: Path Reset ---${RESET}" | tee -a "$LOG_FILE"
    local repo="path_reset_repo"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Content C1" "Commit C1"
    C1_OID=$(get_oid "$repo")
    create_commit "$repo" "file2.txt" "Content C2" "Commit C2"
    C2_OID=$(get_oid "$repo")
    # Modify file1 and add file3 for C3
    echo "Modified file1 C3" > "$repo/file1.txt"
    echo "New file3 C3" > "$repo/file3.txt"
    run_cmd "$repo" add file1.txt file3.txt
    # --- CORECȚIA ESTE AICI (în caz că era greșit și aici, deși problema inițială era în setup_standard_repo) ---
    run_cmd "$repo" commit -m "Commit C3" # Direct commit call after add
    C3_OID=$(get_oid "$repo")
    # Unstaged change AFTER C3
    echo "Unstaged change file1" >> "$repo/file1.txt"

    # Reset only file1.txt in the index to its state in C1
    run_cmd "$repo" reset "$C1_OID" -- file1.txt

    assert_head_is "$repo" "$C3_OID" "Path Reset: HEAD should still be C3"
    # Check index state: file1 should match C1, file2 should match C2, file3 should match C3
    assert_index_differs_from_head "$repo" "true" "Path Reset: Index should differ from HEAD (C3) because file1 was reset"

    # Verify working directory remains untouched by path reset
    assert_file_exists "$repo" "file1.txt" "Path Reset: Working dir file1.txt should exist"
    assert_file_exists "$repo" "file2.txt" "Path Reset: Working dir file2.txt should exist"
    assert_file_exists "$repo" "file3.txt" "Path Reset: Working dir file3.txt should exist"
    assert_file_contains "$repo" "file1.txt" "Unstaged change file1" "Path Reset: Working dir file1.txt should contain unstaged change"
    assert_file_contains "$repo" "file3.txt" "New file3 C3" "Path Reset: Working dir file3.txt should contain C3 content"
    cd "$TEST_DIR"
}


# --- Run Tests ---
test_soft_reset
test_mixed_reset
test_hard_reset
test_path_reset

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
# Log file remains in tests/integration/reset_tests.log

# Exit with status code indicating failure if any tests failed
if [ "$TESTS_FAILED" -gt 0 ]; then
    exit 1
else
    exit 0
fi