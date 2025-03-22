#!/bin/bash
# Test suite for the ASH branch and checkout commands
# This script tests various edge cases and common operations

# Find the ASH executable
if [ -n "$1" ]; then
    # If a path is provided as an argument, use it
    ASH_EXECUTABLE="$1"
elif [ -f "./target/release/AsheraFlow" ]; then
    # Look in standard location after cargo build --release
    ASH_EXECUTABLE="$(pwd)/target/release/AsheraFlow"
elif [ -f "./target/debug/AsheraFlow" ]; then
    # Look in standard location after cargo build
    ASH_EXECUTABLE="$(pwd)/target/debug/AsheraFlow"
else
    echo "ASH executable not found. Please provide the path as an argument."
    echo "Usage: $0 [path-to-ash-executable]"
    exit 1
fi

echo "Using ASH executable: $ASH_EXECUTABLE"

set -e  # Exit on error
TESTS_PASSED=0
TESTS_FAILED=0
VERBOSE=true  # Set to true for detailed debugging output

# Colors for output
RED="\033[0;31m"
GREEN="\033[0;32m"
YELLOW="\033[0;33m"
BLUE="\033[0;34m"
RESET="\033[0m"

# Create temporary directory for testing
TEST_DIR=$(mktemp -d)
echo -e "${BLUE}Using temporary directory: ${TEST_DIR}${RESET}"
cd "$TEST_DIR"

# Helper functions
function setup_repo() {
    rm -rf .ash test_repo 2>/dev/null || true
    mkdir -p test_repo
    cd test_repo
    "$ASH_EXECUTABLE" init . > /dev/null
    echo "setup_repo: ASH repository initialized in $PWD"
}

function create_initial_commit() {
    echo "Initial content" > file1.txt
    "$ASH_EXECUTABLE" add file1.txt > /dev/null
    "$ASH_EXECUTABLE" commit -m "Initial commit" > /dev/null
    echo "create_initial_commit: Created initial commit with file1.txt"
    
    # Get the commit hash for later reference
    INITIAL_COMMIT=$(cat .ash/refs/heads/master)
    echo "Initial commit hash: $INITIAL_COMMIT"
}

function debug_info() {
    if [ "$VERBOSE" = true ]; then
        echo -e "${BLUE}=== Debug Info ===${RESET}"
        echo "Current directory: $(pwd)"
        echo "Files in directory:"
        ls -la
        echo "Content of .ash/HEAD:"
        cat .ash/HEAD 2>/dev/null || echo "HEAD file not found"
        echo "ASH Status:"
        "$ASH_EXECUTABLE" status
        echo -e "${BLUE}================${RESET}"
    fi
}

function assert_success() {
    local cmd="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}"
    if eval "$cmd" > /dev/null 2>&1; then
        echo -e "${GREEN}PASS: $msg${RESET}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}FAIL: $msg${RESET}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

function assert_failure() {
    local cmd="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}"
    if ! eval "$cmd" > /dev/null 2>&1; then
        echo -e "${GREEN}PASS: $msg${RESET}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}FAIL: $msg${RESET}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

function assert_file_exists() {
    local file="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}"
    if [ -f "$file" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}FAIL: $msg - File $file does not exist${RESET}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

function assert_file_not_exists() {
    local file="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}"
    if [ ! -f "$file" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}FAIL: $msg - File $file exists when it shouldn't${RESET}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

function assert_branch_exists() {
    local branch="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}"
    if [ -f ".ash/refs/heads/$branch" ]; then
        echo -e "${GREEN}PASS: $msg${RESET}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}FAIL: $msg - Branch $branch does not exist${RESET}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

function assert_branch_is_current() {
    local branch="$1"
    local msg="$2"
    echo -e "${YELLOW}TEST: $msg${RESET}"
    local head_content=$(cat .ash/HEAD)
    if [[ "$head_content" == *"$branch"* ]]; then
        echo -e "${GREEN}PASS: $msg${RESET}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}FAIL: $msg - Current branch is not $branch${RESET}"
        echo "HEAD content: $head_content"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

function get_commit_hash() {
    cat ".ash/refs/heads/$1" 2>/dev/null || echo ""
}

# ================================================
# FOCUSED DEBUG TEST - BRANCH WITH START POINT
# ================================================

# Detailed debug test for the problematic test case
function debug_branch_with_start_point() {
    echo -e "${BLUE}=== DEBUGGING TEST: Creating branch from specific commit ===${RESET}"
    
    setup_repo
    create_initial_commit
    
    # Store the initial commit hash for later reference
    local first_commit=$(cat .ash/refs/heads/master)
    echo "Initial commit hash: $first_commit"
    
    # Get the list of files in the working tree after initial commit
    echo "Files after initial commit:"
    ls -la
    
    # Show the content of the first commit
    echo "Content of initial commit:"
    "$ASH_EXECUTABLE" log -1
    
    # Create a second commit on master
    echo "Creating second commit..."
    echo "Second file" > file2.txt
    "$ASH_EXECUTABLE" add file2.txt
    "$ASH_EXECUTABLE" commit -m "Second commit"
    
    # Now we have two commits - get the second commit hash
    local second_commit=$(cat .ash/refs/heads/master)
    echo "Second commit hash: $second_commit"
    
    # Show all the files in the working tree after second commit
    echo "Files after second commit:"
    ls -la
    
    # Check the commit history
    echo "Commit history after second commit:"
    "$ASH_EXECUTABLE" log
    
    # Create a branch from the first commit
    echo "Creating branch 'old-branch' from first commit..."
    "$ASH_EXECUTABLE" branch old-branch $first_commit
    
    # Verify the branch was created
    if [ -f ".ash/refs/heads/old-branch" ]; then
        echo "PASS: Branch 'old-branch' created successfully"
        
        # Verify the branch points to the first commit
        local branch_commit=$(cat .ash/refs/heads/old-branch)
        echo "old-branch commit hash: $branch_commit"
        
        if [ "$branch_commit" = "$first_commit" ]; then
            echo "PASS: old-branch points to the first commit"
        else
            echo "FAIL: old-branch points to $branch_commit instead of $first_commit"
        fi
    else
        echo "FAIL: Branch 'old-branch' not created"
    fi
    
    # Show the current branch and working tree state
    echo "Current branch before checkout:"
    head_content=$(cat .ash/HEAD)
    echo "$head_content"
    
    echo "Working tree before checkout to old-branch:"
    ls -la
    
    # Check the index
    echo "Index entries before checkout:"
    "$ASH_EXECUTABLE" status
    
    # Checkout the branch
    echo "Checking out old-branch..."
    checkout_output=$("$ASH_EXECUTABLE" checkout old-branch 2>&1)
    checkout_status=$?
    
    echo "Checkout output: $checkout_output"
    echo "Checkout status: $checkout_status"
    
    # Check the current branch after checkout
    echo "Current branch after checkout:"
    head_content=$(cat .ash/HEAD)
    echo "$head_content"
    
    # Check the working tree after checkout
    echo "Working tree after checkout to old-branch:"
    ls -la
    
    # Verify file2.txt doesn't exist
    if [ -f "file2.txt" ]; then
        echo "FAIL: file2.txt exists when it shouldn't!"
        echo "Content of file2.txt:"
        cat file2.txt
    else
        echo "PASS: file2.txt is not present, as expected"
    fi
    
    # Try an explicit checkout of file1.txt to verify checkout works
    echo "Testing explicit checkout of file1.txt..."
    "$ASH_EXECUTABLE" checkout -- file1.txt
    
    # Test switching back to master
    echo "Switching back to master..."
    "$ASH_EXECUTABLE" checkout master
    
    # Verify file2.txt exists again
    echo "Working tree after checkout to master:"
    ls -la
    
    if [ -f "file2.txt" ]; then
        echo "PASS: file2.txt exists again on master"
    else
        echo "FAIL: file2.txt is missing on master"
    fi
    
    echo -e "${BLUE}=== DEBUG TEST COMPLETE ===${RESET}"
}

# ================================================
# MAIN TEST SECTION
# ================================================

echo -e "${BLUE}=== Running focused debug test on the problematic test case ===${RESET}"
debug_branch_with_start_point

# Print summary
echo -e "${BLUE}=== Debug Test Complete ===${RESET}"
echo "Check the output above for detailed information about the problematic test case."

# Clean up
cd ..
rm -rf "$TEST_DIR"

exit 0