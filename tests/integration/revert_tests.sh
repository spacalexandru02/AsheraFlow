#!/bin/bash
# Test suite for the ASH revert command

# --- Configuration ---
# Find the ASH executable (same logic as reset_tests.sh)
if [ -n "$1" ]; then
    ASH_EXECUTABLE="$1"
elif [ -f "./target/release/AsheraFlow" ]; then
    ASH_EXECUTABLE="$(pwd)/target/release/AsheraFlow"
elif [ -f "./target/debug/AsheraFlow" ]; then
    ASH_EXECUTABLE="$(pwd)/target/debug/AsheraFlow"
elif [ -f "../../target/release/AsheraFlow" ]; then
    ASH_EXECUTABLE="$(pwd)/../../target/release/AsheraFlow"
elif [ -f "../../target/debug/AsheraFlow" ]; then
    ASH_EXECUTABLE="$(pwd)/../../target/debug/AsheraFlow"
else
    echo "ASH executable not found. Build the project or provide the path as an argument."
    echo "Usage: $0 [path-to-ash-executable] (run from project root or tests/integration)"
    exit 1
fi
ASH_EXECUTABLE=$(cd "$(dirname "$ASH_EXECUTABLE")" && pwd)/$(basename "$ASH_EXECUTABLE")

# --- Logging Setup ---
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
LOG_FILE="$SCRIPT_DIR/revert_tests.log"
> "$LOG_FILE"
echo "Using ASH executable: $ASH_EXECUTABLE" | tee -a "$LOG_FILE"
ASH_CMD="$ASH_EXECUTABLE"

set -e # Exit immediately if a command exits with a non-zero status.
# set -x # Uncomment for detailed command execution debugging

# --- Test Environment Setup ---
TEST_DIR=$(mktemp -d)
echo "Using temporary directory for test repos: ${TEST_DIR}" | tee -a "$LOG_FILE"
echo "Logging detailed output to: ${LOG_FILE}" | tee -a "$LOG_FILE"
ORIGINAL_PWD=$(pwd)
cd "$TEST_DIR" || exit 1

# --- Colors and Counters ---
RED="\033[0;31m"
GREEN="\033[0;32m"
YELLOW="\033[0;33m"
BLUE="\033[0;34m"
RESET="\033[0m"
TESTS_PASSED=0
TESTS_FAILED=0

# --- Helper Functions (Copied & Adapted from reset_tests.sh) ---
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
    # Log initialization message (to log only)
    echo -e "[INFO] Initialized repo in $(pwd)" >> "$LOG_FILE"
    cd .. # Go back to TEST_DIR
}

function create_commit() {
    local repo_name="$1"
    local file_name="$2"
    local content="$3"
    local message="$4"
    local branch
    branch=$(cd "$repo_name" && cat .ash/HEAD 2>/dev/null | sed 's|ref: refs/heads/||' || echo "master")
    echo "$content" > "$repo_name/$file_name"
    # Redirect add/commit output to log file
    (cd "$repo_name" && "$ASH_CMD" add "$file_name" >> "$LOG_FILE" 2>&1) || { echo "[ERROR] Add failed in create_commit" >> "$LOG_FILE"; exit 1; }
    (cd "$repo_name" && "$ASH_CMD" commit -m "$message" >> "$LOG_FILE" 2>&1) || { echo "[ERROR] Commit failed in create_commit" >> "$LOG_FILE"; exit 1; }
    # Log commit message (to log only)
    echo "[INFO] Commit on '$branch': '$message' ($file_name)" >> "$LOG_FILE"
}

# run_cmd adapted to check for EXPECTED failure
function run_cmd() {
    local repo_name="$1"
    local expect_fail=${2:-"false"} # Second argument indicates if failure is expected
    shift 2 # Remove repo_name and expect_fail from args
    # Log command to both console and file
    echo -e "${YELLOW}  CMD [in $repo_name]: ${ASH_CMD} $@${RESET}" | tee -a "$LOG_FILE"
    local cmd_output_file
    cmd_output_file=$(mktemp)
    local full_cmd_line # Variable to store the full command with resolved variables

    # Build the full command line string for logging, ensuring variables are expanded
    local expanded_args=()
    for arg in "$@"; do
        expanded_args+=("$(printf '%q' "$arg")") # Quote arguments safely for echo
    done
    full_cmd_line="${ASH_CMD} ${expanded_args[*]}"
    # echo "[DEBUG] Executing: $full_cmd_line in $repo_name" >> "$LOG_FILE" # Log the expanded command

    if (cd "$repo_name" && "$ASH_CMD" "$@") >> "$cmd_output_file" 2>&1; then
        cat "$cmd_output_file" >> "$LOG_FILE" # Append output to main log
        rm "$cmd_output_file"
        if [[ "$expect_fail" == "true" ]]; then
            echo -e "${RED}  CMD UNEXPECTEDLY SUCCEEDED (expected failure)${RESET}" | tee -a "$LOG_FILE"
            return 1 # Failure for the test case
        else
            echo -e "${GREEN}  CMD OK${RESET}" | tee -a "$LOG_FILE"
            return 0 # Success
        fi
    else
        local exit_code=$?
        cat "$cmd_output_file" >> "$LOG_FILE" # Append output to main log
        if [[ "$expect_fail" == "true" ]]; then
             echo -e "${GREEN}  CMD FAILED AS EXPECTED (Exit Code: $exit_code)${RESET}" | tee -a "$LOG_FILE"
             # Optionally show output on console for expected failure
             # echo "--- Expected Failure Output ---"
             # cat "$LOG_FILE" | tail -n 10 # Show recent log lines relevant to the command
             # echo "--- End Expected Failure Output ---"
             rm "$cmd_output_file"
             return 0 # Success for the test case
        else
            echo -e "${RED}  CMD FAILED UNEXPECTEDLY (Exit Code: $exit_code)${RESET}" | tee -a "$LOG_FILE"
            echo -e "${RED}--- Start Log Output Snippet [${LOG_FILE}] ---${RESET}"
            tail -n 50 "$LOG_FILE" # Show recent log lines
            echo -e "${RED}--- End Log Output Snippet ---${RESET}"
            rm "$cmd_output_file"
            return $exit_code # Failure for the test case
        fi
    fi
}

# --- Corecție Finală: Funcția get_oid scrie DOAR OID/unknown_oid pe stdout ---
function get_oid() {
    local repo_name="$1"
    local oid_line
    # --- Scrie debug DOAR în fișierul log ---
    echo "[DEBUG] Running log in get_oid for $repo_name" >> "$LOG_FILE"

    # Rulează comanda și capturează stdout ȘI stderr împreună
    oid_line=$( (cd "$repo_name" && "$ASH_CMD" log --oneline | head -n 1) 2>&1 )
    local exit_code=$? # Capturează exit code-ul comenzii (al `head` în acest caz)

    # --- Scrie debug DOAR în fișierul log ---
    echo "[DEBUG] log exit code: [$exit_code], output line: [$oid_line]" >> "$LOG_FILE"

    # Verifică exit code și dacă output-ul conține erori comune sau e gol
    if [[ "$exit_code" -ne 0 || -z "$oid_line" || "$oid_line" == *"Error"* || "$oid_line" == *"fatal"* || "$oid_line" == *"no commits yet"* ]]; then
        # --- Scrie debug DOAR în fișierul log ---
        echo "[DEBUG] get_oid failed or got no commits for $repo_name. Log output: $oid_line" >> "$LOG_FILE"
        # --- Trimite "unknown_oid" pe stdout în caz de eroare ---
        echo "unknown_oid"
    else
        # Extrage și trimite DOAR OID-ul (primul câmp) pe stdout
        echo "$oid_line" | cut -d ' ' -f 1
    fi
}

# --- Restul funcțiilor helper (assert_*, resolve_conflict) rămân la fel ---

function get_file_oid_from_index() {
    local repo_name="$1"
    local file_path="$2"
    (cd "$repo_name" && "$ASH_CMD" status --porcelain | grep " $file_path$" | awk '{print $3}') 2>/dev/null || echo "not_in_index"
}

function assert_file_exists() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ -f "$repo_name/$file_path" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' does not exist.${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_file_not_exists() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ ! -f "$repo_name/$file_path" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' exists when it shouldn't.${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_file_contains() {
    local repo_name="$1"
    local file_path="$2"
    local expected_content="$3"
    local msg="$4"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ ! -f "$repo_name/$file_path" ]; then
        echo -e "${RED}FAIL: $msg - File '$repo_name/$file_path' does not exist to check content.${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1)); return 1
    fi
    local actual_content
    actual_content=$(cat "$repo_name/$file_path")
    if [[ "$actual_content" == "$expected_content" ]]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Content mismatch in '$repo_name/$file_path'.${RESET}" | tee -a "$LOG_FILE"
        echo "--- Expected ---" >> "$LOG_FILE"; echo "$expected_content" >> "$LOG_FILE"
        echo "--- Actual ---" >> "$LOG_FILE"; echo "$actual_content" >> "$LOG_FILE"; echo "--- End Actual ---" >> "$LOG_FILE"
        echo "Expected:" $'\n'"$expected_content"$'\n'"Actual:"$'\n'"$actual_content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_head_is() {
    local repo_name="$1"
    local expected_oid="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [[ -z "$expected_oid" || "$expected_oid" == "unknown_oid" ]]; then
        echo -e "${RED}FAIL: $msg - Bash variable for expected OID is invalid ('$expected_oid')! Check OID capture.${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1)); return 1
    fi
    local head_ref_content
    head_ref_content=$(cat "$repo_name/.ash/HEAD" 2>/dev/null)
    local actual_oid=""
    if [[ "$head_ref_content" == ref:* ]]; then
        local ref_path=${head_ref_content#ref: }; ref_path=$(echo "$ref_path" | xargs)
        if [[ -f "$repo_name/.ash/$ref_path" ]]; then actual_oid=$(cat "$repo_name/.ash/$ref_path" 2>/dev/null); else actual_oid="<broken_ref:$ref_path>"; fi
    else actual_oid="$head_ref_content"; fi

    if [ "$actual_oid" == "$expected_oid" ]; then
        echo -e "${GREEN}PASS: $msg (HEAD is $actual_oid)${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Expected HEAD OID '$expected_oid', but got '$actual_oid'. HEAD content: '$head_ref_content' ${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_commit_message_contains() {
    local repo_name="$1"
    local commit_ref="$2" # e.g., HEAD
    local expected_text="$3"
    local msg="$4"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    local commit_message
    commit_message=$( (cd "$repo_name" && "$ASH_CMD" log -n 1 --pretty=format:%B "$commit_ref") 2>/dev/null || echo "Log command failed" )
    if [[ "$commit_message" == *"$expected_text"* ]]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Commit message for '$commit_ref' did not contain '$expected_text'. Message:\n$commit_message${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_conflict_markers_in() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if grep -q '<<<<<<<' "$repo_name/$file_path" && grep -q '=======' "$repo_name/$file_path" && grep -q '>>>>>>>' "$repo_name/$file_path"; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Conflict markers not found in '$repo_name/$file_path'. Content:${RESET}" | tee -a "$LOG_FILE"
        cat "$repo_name/$file_path" >> "$LOG_FILE"; cat "$repo_name/$file_path"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_no_conflict_markers_in() {
    local repo_name="$1"
    local file_path="$2"
    local msg="$3"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if ! grep -q '<<<<<<<' "$repo_name/$file_path" && ! grep -q '=======' "$repo_name/$file_path" && ! grep -q '>>>>>>>' "$repo_name/$file_path"; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Conflict markers unexpectedly found in '$repo_name/$file_path'. Content:${RESET}" | tee -a "$LOG_FILE"
        cat "$repo_name/$file_path" >> "$LOG_FILE"; cat "$repo_name/$file_path"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_revert_state_exists() {
    local repo_name="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ -d "$repo_name/.ash/revert" ] && [ -f "$repo_name/.ash/revert/message" ] && [ -f "$repo_name/.ash/revert/commit" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Directory or files in '$repo_name/.ash/revert' not found.${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function assert_revert_state_not_exists() {
    local repo_name="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}" | tee -a "$LOG_FILE"
    if [ ! -d "$repo_name/.ash/revert" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}FAIL: $msg - Directory '$repo_name/.ash/revert' unexpectedly exists.${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

function resolve_conflict() {
    local repo_name="$1"
    local file_path="$2"
    local resolved_content="$3"
    echo "[INFO] Resolving conflict in $repo_name/$file_path..." >> "$LOG_FILE"
    # Simple resolution: just overwrite the file
    echo "$resolved_content" > "$repo_name/$file_path"
    echo "[INFO] Conflict resolved." >> "$LOG_FILE"
}


# --- Test Cases ---

function test_simple_revert() {
    echo -e "\n${BLUE}--- Test: Simple Revert (No Conflict) ---${RESET}" | tee -a "$LOG_FILE"
    local repo="simple_revert_repo"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Line 1\nLine 2\nLine 3" "Commit C1"
    local C1_OID=$(get_oid "$repo")
    create_commit "$repo" "file1.txt" "Line 1 MODIFIED\nLine 2\nLine 3" "Commit C2"
    local C2_OID=$(get_oid "$repo")
    create_commit "$repo" "file2.txt" "New file content" "Commit C3"
    local C3_OID=$(get_oid "$repo")

    # Verifica OID-urile capturate
    if [[ "$C1_OID" == "unknown_oid" || "$C2_OID" == "unknown_oid" || "$C3_OID" == "unknown_oid" ]]; then
        echo -e "${RED}FAIL: Failed to capture OIDs for setup in $repo${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED+5)); return 1
    fi
    echo "[DEBUG] OIDs for $repo: C1=$C1_OID, C2=$C2_OID, C3=$C3_OID" >> "$LOG_FILE"

    # Revert C2
    run_cmd "$repo" false revert "$C2_OID"
    local REVERT_C2_OID=$(get_oid "$repo")
     if [[ "$REVERT_C2_OID" == "unknown_oid" || "$REVERT_C2_OID" == "$C3_OID" ]]; then # Check it's a new commit
        echo -e "${RED}FAIL: Failed to get new OID after revert C2 in $repo${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED+5)); return 1
    fi
     echo "[DEBUG] Revert C2 OID in $repo: $REVERT_C2_OID" >> "$LOG_FILE"


    # Checks
    assert_head_is "$repo" "$REVERT_C2_OID" "Simple Revert: HEAD should be the new revert commit"
    local parent_of_revert
    parent_of_revert=$( (cd "$repo" && "$ASH_CMD" log --pretty=format:%P -n 1 "$REVERT_C2_OID") 2>/dev/null )
    if [[ "$parent_of_revert" != "$C3_OID" ]]; then
        echo -e "${RED}FAIL: Simple Revert: Parent of revert commit should be C3 ($C3_OID), but was '$parent_of_revert'${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
    else
         echo -e "${GREEN}PASS: Simple Revert: Parent of revert commit is C3${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
    fi
    # Need to use log -n 1 because get_oid might fail here if revert doesn't implement log options yet
    assert_commit_message_contains "$repo" HEAD "Revert \"Commit C2\"" "Simple Revert: Commit message should indicate revert of C2"
    assert_commit_message_contains "$repo" HEAD "This reverts commit $C2_OID" "Simple Revert: Commit message should contain reverted OID"
    assert_file_exists "$repo" "file1.txt" "Simple Revert: file1.txt should exist"
    assert_file_exists "$repo" "file2.txt" "Simple Revert: file2.txt should exist (from C3)"
    assert_file_contains "$repo" "file1.txt" "$(echo -e "Line 1\nLine 2\nLine 3")" "Simple Revert: file1.txt content should be back to C1 state"
    assert_file_contains "$repo" "file2.txt" "New file content" "Simple Revert: file2.txt content should be unchanged from C3"
    assert_revert_state_not_exists "$repo" "Simple Revert: Revert state dir should not exist"
    cd "$TEST_DIR"
}

function test_revert_conflict_setup() {
    local repo="$1"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Line A\nLine B\nLine C" "Commit C1"
    local C1_OID=$(get_oid "$repo")
    create_commit "$repo" "file1.txt" "Line A\nLine B MODIFIED by C2\nLine C" "Commit C2"
    local C2_OID=$(get_oid "$repo")
    create_commit "$repo" "file1.txt" "Line A\nLine B MODIFIED by C3\nLine C" "Commit C3"
    local C3_OID=$(get_oid "$repo")
    # Verifica OID-urile capturate
     if [[ "$C1_OID" == "unknown_oid" || "$C2_OID" == "unknown_oid" || "$C3_OID" == "unknown_oid" ]]; then
         echo "[ERROR] Failed to capture OIDs for setup in $repo" >> "$LOG_FILE"; echo "unknown_oid unknown_oid unknown_oid"; return 1
     fi
    echo "$C1_OID $C2_OID $C3_OID" # Return OIDs
}

function test_revert_conflict() {
    echo -e "\n${BLUE}--- Test: Revert Causing Conflict ---${RESET}" | tee -a "$LOG_FILE"
    local repo="revert_conflict_repo"
    local C1_OID C2_OID C3_OID
    read -r C1_OID C2_OID C3_OID < <(test_revert_conflict_setup "$repo")
    if [[ "$C1_OID" == "unknown_oid" ]]; then echo -e "${RED}FAIL: Setup failed for $repo ${RESET}"; TESTS_FAILED=$((TESTS_FAILED+3)); return 1; fi
    echo "[DEBUG] OIDs for $repo: C1=$C1_OID, C2=$C2_OID, C3=$C3_OID" >> "$LOG_FILE"

    # Revert C2 - Expecting failure
    run_cmd "$repo" true revert "$C2_OID"

    # Checks
    assert_head_is "$repo" "$C3_OID" "Revert Conflict: HEAD should remain at C3"
    assert_conflict_markers_in "$repo" "file1.txt" "Revert Conflict: file1.txt should contain conflict markers"
    assert_revert_state_exists "$repo" "Revert Conflict: Revert state dir should exist"
    cd "$TEST_DIR"
}

function test_revert_continue() {
    echo -e "\n${BLUE}--- Test: Revert --continue ---${RESET}" | tee -a "$LOG_FILE"
    local repo="revert_continue_repo"
    local C1_OID C2_OID C3_OID
    read -r C1_OID C2_OID C3_OID < <(test_revert_conflict_setup "$repo")
     if [[ "$C1_OID" == "unknown_oid" ]]; then echo -e "${RED}FAIL: Setup failed for $repo ${RESET}"; TESTS_FAILED=$((TESTS_FAILED+5)); return 1; fi
     echo "[DEBUG] OIDs for $repo: C1=$C1_OID, C2=$C2_OID, C3=$C3_OID" >> "$LOG_FILE"

    # Revert C2 - Expecting failure
    run_cmd "$repo" true revert "$C2_OID"
    assert_revert_state_exists "$repo" "Revert Continue: Revert state should exist initially"

    # Resolve conflict and continue
    resolve_conflict "$repo" "file1.txt" "Line A\nLine B RESOLVED\nLine C"
    run_cmd "$repo" false add "file1.txt" # Stage the resolved file
    run_cmd "$repo" false revert --continue
    local REVERT_C2_OID=$(get_oid "$repo")
     if [[ "$REVERT_C2_OID" == "unknown_oid" || "$REVERT_C2_OID" == "$C3_OID" ]]; then
        echo -e "${RED}FAIL: Failed to get new OID after revert --continue in $repo${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED+5)); return 1
    fi
     echo "[DEBUG] Revert Continue OID in $repo: $REVERT_C2_OID" >> "$LOG_FILE"


    # Checks
    assert_head_is "$repo" "$REVERT_C2_OID" "Revert Continue: HEAD should be the new revert commit"
    local parent_of_revert
    parent_of_revert=$( (cd "$repo" && "$ASH_CMD" log --pretty=format:%P -n 1 "$REVERT_C2_OID") 2>/dev/null )
     if [[ "$parent_of_revert" != "$C3_OID" ]]; then
         echo -e "${RED}FAIL: Revert Continue: Parent of revert commit should be C3 ($C3_OID), but was '$parent_of_revert'${RESET}" | tee -a "$LOG_FILE"; TESTS_FAILED=$((TESTS_FAILED + 1))
     else
          echo -e "${GREEN}PASS: Revert Continue: Parent of revert commit is C3${RESET}" | tee -a "$LOG_FILE"; TESTS_PASSED=$((TESTS_PASSED + 1))
     fi
    assert_commit_message_contains "$repo" HEAD "Revert \"Commit C2\"" "Revert Continue: Commit message should indicate revert of C2"
    assert_commit_message_contains "$repo" HEAD "This reverts commit $C2_OID" "Revert Continue: Commit message should contain reverted OID"
    assert_file_contains "$repo" "file1.txt" "Line A\nLine B RESOLVED\nLine C" "Revert Continue: file1.txt should contain resolved content"
    assert_no_conflict_markers_in "$repo" "file1.txt" "Revert Continue: Conflict markers should be gone"
    assert_revert_state_not_exists "$repo" "Revert Continue: Revert state dir should be removed"
    cd "$TEST_DIR"
}

function test_revert_abort() {
    echo -e "\n${BLUE}--- Test: Revert --abort ---${RESET}" | tee -a "$LOG_FILE"
    local repo="revert_abort_repo"
    local C1_OID C2_OID C3_OID
    read -r C1_OID C2_OID C3_OID < <(test_revert_conflict_setup "$repo")
    if [[ "$C1_OID" == "unknown_oid" ]]; then echo -e "${RED}FAIL: Setup failed for $repo ${RESET}"; TESTS_FAILED=$((TESTS_FAILED+4)); return 1; fi
    echo "[DEBUG] OIDs for $repo: C1=$C1_OID, C2=$C2_OID, C3=$C3_OID" >> "$LOG_FILE"

    # Revert C2 - Expecting failure
    run_cmd "$repo" true revert "$C2_OID"
    assert_revert_state_exists "$repo" "Revert Abort: Revert state should exist initially"
    assert_conflict_markers_in "$repo" "file1.txt" "Revert Abort: file1.txt has conflicts before abort"

    # Abort the revert
    run_cmd "$repo" false revert --abort

    # Checks
    assert_head_is "$repo" "$C3_OID" "Revert Abort: HEAD should be back at C3"
    # Check file content is back to C3 state
    assert_file_contains "$repo" "file1.txt" "Line A\nLine B MODIFIED by C3\nLine C" "Revert Abort: file1.txt content should be back to C3 state"
    assert_no_conflict_markers_in "$repo" "file1.txt" "Revert Abort: Conflict markers should be gone after abort"
    assert_revert_state_not_exists "$repo" "Revert Abort: Revert state dir should be removed"
    cd "$TEST_DIR"
}

function test_revert_root() {
    echo -e "\n${BLUE}--- Test: Revert Root Commit ---${RESET}" | tee -a "$LOG_FILE"
    local repo="revert_root_repo"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Content C1" "Commit C1"
    local C1_OID=$(get_oid "$repo")
    if [[ "$C1_OID" == "unknown_oid" ]]; then echo -e "${RED}FAIL: Failed to capture OID for C1 in $repo ${RESET}"; TESTS_FAILED=$((TESTS_FAILED+1)); return 1; fi
    echo "[DEBUG] OID for $repo: C1=$C1_OID" >> "$LOG_FILE"

    # Revert C1 - Expecting failure
    run_cmd "$repo" true revert "$C1_OID"

    # Check HEAD is still C1
    assert_head_is "$repo" "$C1_OID" "Revert Root: HEAD should remain at C1"
    cd "$TEST_DIR"
}

function test_revert_uncommitted() {
    echo -e "\n${BLUE}--- Test: Revert with Uncommitted Changes ---${RESET}" | tee -a "$LOG_FILE"
    local repo="revert_uncommitted_repo"
    setup_repo "$repo"
    create_commit "$repo" "file1.txt" "Content C1" "Commit C1"
    local C1_OID=$(get_oid "$repo")
    create_commit "$repo" "file2.txt" "Content C2" "Commit C2"
    local C2_OID=$(get_oid "$repo")
     if [[ "$C1_OID" == "unknown_oid" || "$C2_OID" == "unknown_oid" ]]; then echo -e "${RED}FAIL: Failed to capture OIDs for setup in $repo ${RESET}"; TESTS_FAILED=$((TESTS_FAILED+2)); return 1; fi
     echo "[DEBUG] OIDs for $repo: C1=$C1_OID, C2=$C2_OID" >> "$LOG_FILE"

    # Modify a file without committing
    echo "Local modification" >> "$repo/file1.txt"

    # Revert C2 - Expecting failure because of uncommitted changes
    run_cmd "$repo" true revert "$C2_OID"

    # Check HEAD is still C2
    assert_head_is "$repo" "$C2_OID" "Revert Uncommitted: HEAD should remain at C2"
    assert_file_contains "$repo" "file1.txt" "$(echo -e "Content C1\nLocal modification")" "Revert Uncommitted: Local modification should persist"
    cd "$TEST_DIR"
}

# --- Run Tests ---
test_simple_revert
test_revert_conflict
test_revert_continue
test_revert_abort
test_revert_root
test_revert_uncommitted

# --- Summary ---
echo -e "\n${BLUE}--- Test Summary ---${RESET}" | tee -a "$LOG_FILE"
echo -e "${GREEN}Tests Passed: $TESTS_PASSED${RESET}" | tee -a "$LOG_FILE"
if [ "$TESTS_FAILED" -gt 0 ]; then
    echo -e "${RED}Tests Failed: $TESTS_FAILED${RESET}" | tee -a "$LOG_FILE"
else
    echo -e "${GREEN}All revert tests passed!${RESET}" | tee -a "$LOG_FILE"
fi

# --- Cleanup ---
cd "$ORIGINAL_PWD"
rm -rf "$TEST_DIR"
echo "Cleaned up temporary directory: $TEST_DIR" | tee -a "$LOG_FILE"

# Exit with status code indicating failure if any tests failed
if [ "$TESTS_FAILED" -gt 0 ]; then
    exit 1
else
    exit 0
fi