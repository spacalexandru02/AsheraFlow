// src/diff/myers.rs
use std::cmp;
use std::collections::HashMap;

/// Represents a single edit operation in a diff
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Insert(usize),  // Insert line at given position in b
    Delete(usize),  // Delete line at given position in a
    Equal(usize, usize), // Lines are equal at given positions in a and b
}

/// Calculates a diff between two sequences of lines using the Myers algorithm
pub fn diff_lines(a: &[String], b: &[String]) -> Vec<Edit> {
    // Get the maximum possible edit distance
    let n = a.len();
    let m = b.len();
    let max_d = n + m;

    // Initialize our table with a single entry
    let mut v = HashMap::new();
    v.insert(1isize, 0usize);  // Initial k=1 position

    // Iterate through each possible edit distance
    for d in 0..=max_d {
        // Try each possible k value for this edit distance
        // We use the range -d as isize..=d as isize to handle negative values properly
        for k in (-1 * d as isize..=d as isize).step_by(2) {
            // Choose whether to move down or right to reach best x value
            let mut x;
            if k == -1 * d as isize || (k != d as isize && v.get(&(k-1)).unwrap_or(&0) < v.get(&(k+1)).unwrap_or(&0)) {
                // Move down (insertion)
                x = *v.get(&(k+1)).unwrap_or(&0);
            } else {
                // Move right (deletion)
                x = *v.get(&(k-1)).unwrap_or(&0) + 1;
            }
            
            // Start position in y
            let mut y = (x as isize - k) as usize;
            
            // Follow diagonal path as far as possible (matching characters)
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            
            // Update table with how far we reached
            v.insert(k, x);
            
            // Check if we've reached the end of both strings
            if x >= n && y >= m {
                // We found the shortest edit script, now backtrack to build the diff
                return backtrack_path(d, k, v);
            }
        }
    }
    
    // Fallback: if no path found, do a complete replacement
    let mut result = Vec::new();
    for i in 0..n {
        result.push(Edit::Delete(i));
    }
    for j in 0..m {
        result.push(Edit::Insert(j));
    }
    result
}

/// Backtrack through the table to reconstruct the edit path
fn backtrack_path(d: usize, k: isize, v: HashMap<isize, usize>) -> Vec<Edit> {
    let mut edit_script = Vec::new();
    let mut x = *v.get(&k).unwrap_or(&0);
    let mut y = (x as isize - k) as usize;
    
    // We need to go backwards from (d,k) to (0,0)
    let mut edits = Vec::new();
    
    // Start with the current position
    edits.push((x, y, k));
    
    // Backtrack to find all moves that led to the final position
    let mut current_k = k;
    for d_step in (1..=d).rev() {
        // Determine whether we got to the current position by moving down or right
        let prev_k;
        if current_k == -1 * d_step as isize || (current_k != d_step as isize && 
            v.get(&(current_k-1)).unwrap_or(&0) < v.get(&(current_k+1)).unwrap_or(&0)) {
            // We moved down, so previous k = k+1
            prev_k = current_k + 1;
        } else {
            // We moved right, so previous k = k-1
            prev_k = current_k - 1;
        }
        
        // Get the x value for the previous position
        let prev_x = *v.get(&prev_k).unwrap_or(&0);
        let prev_y = (prev_x as isize - prev_k) as usize;
        
        // Add the move to our list
        edits.push((prev_x, prev_y, prev_k));
        
        // Update current position for next iteration
        current_k = prev_k;
    }
    
    // Now construct the actual diff by going forwards through the moves
    edits.reverse();
    
    // Process pairs of edit steps to determine the edits
    let mut i = 0;
    while i < edits.len() - 1 {
        let (curr_x, curr_y, _) = edits[i];
        let (next_x, next_y, _) = edits[i + 1];
        
        if next_x > curr_x && next_y > curr_y {
            // We moved diagonally, so lines are equal
            // Find how far diagonally we moved
            let diag_steps = cmp::min(next_x - curr_x, next_y - curr_y);
            for j in 0..diag_steps {
                edit_script.push(Edit::Equal(curr_x + j, curr_y + j));
            }
            
            if next_x - curr_x > diag_steps {
                // We also had a horizontal move (deletion)
                edit_script.push(Edit::Delete(next_x - 1));
            } else if next_y - curr_y > diag_steps {
                // We also had a vertical move (insertion)
                edit_script.push(Edit::Insert(next_y - 1));
            }
        } else if next_x > curr_x {
            // We moved horizontally (deletion)
            edit_script.push(Edit::Delete(curr_x));
        } else if next_y > curr_y {
            // We moved vertically (insertion)
            edit_script.push(Edit::Insert(curr_y));
        }
        
        i += 1;
    }
    
    edit_script
}

/// Format a diff for display, git-style
pub fn format_diff(a: &[String], b: &[String], edits: &[Edit], context_lines: usize) -> String {
    let mut result = String::new();
    
    // Special case for empty files
    if a.is_empty() && !b.is_empty() {
        // Adding all content to an empty file
        result.push_str("@@ -0,0 +1,");
        result.push_str(&b.len().to_string());
        result.push_str(" @@\n");
        
        for line in b {
            result.push_str(&format!("+{}\n", line));
        }
        
        return result;
    } else if !a.is_empty() && b.is_empty() {
        // Removing all content
        result.push_str("@@ -1,");
        result.push_str(&a.len().to_string());
        result.push_str(" +0,0 @@\n");
        
        for line in a {
            result.push_str(&format!("-{}\n", line));
        }
        
        return result;
    } else if a.is_empty() && b.is_empty() {
        // Both files are empty
        return result;
    }
    
    // Group edits into "hunks" - consecutive non-equal operations with context
    let mut hunks = Vec::new();
    let mut current_hunk = Vec::new();
    let mut last_op_idx = 0;
    
    for (i, edit) in edits.iter().enumerate() {
        match edit {
            Edit::Equal(_, _) => {
                // If we have pending edits and this equal is far enough from the last edit
                if !current_hunk.is_empty() && i - last_op_idx > context_lines * 2 {
                    // Finish the current hunk with context lines
                    for j in 0..context_lines {
                        if last_op_idx + j + 1 < edits.len() {
                            if let Edit::Equal(_, _) = edits[last_op_idx + j + 1] {
                                current_hunk.push(last_op_idx + j + 1);
                            }
                        }
                    }
                    
                    hunks.push(current_hunk);
                    current_hunk = Vec::new();
                    
                    // Start a new hunk with context lines
                    if i >= context_lines {
                        for j in 0..context_lines {
                            if i >= j && i - j < edits.len() {
                                if let Edit::Equal(_, _) = edits[i - j] {
                                    current_hunk.push(i - j);
                                }
                            }
                        }
                    }
                }
            },
            Edit::Insert(_) | Edit::Delete(_) => {
                current_hunk.push(i);
                last_op_idx = i;
            }
        }
    }
    
    // Add any remaining edits to the final hunk
    if !current_hunk.is_empty() {
        // Add trailing context
        for j in 0..context_lines {
            if last_op_idx + j + 1 < edits.len() {
                if let Edit::Equal(_, _) = edits[last_op_idx + j + 1] {
                    current_hunk.push(last_op_idx + j + 1);
                }
            }
        }
        hunks.push(current_hunk);
    }
    
    // Format each hunk
    for hunk in hunks {
        if hunk.is_empty() {
            continue;
        }
        
        // Calculate hunk header information
        let (a_start, a_count, b_start, b_count) = calculate_hunk_range(a, b, edits, &hunk);
        
        // Add the hunk header
        result.push_str(&format!("@@ -{},{} +{},{} @@\n", 
                              a_start + 1, a_count, b_start + 1, b_count));
        
        // Format the lines in the hunk
        for &edit_idx in &hunk {
            match &edits[edit_idx] {
                Edit::Equal(a_idx, _b_idx) => {
                    if *a_idx < a.len() {
                        result.push_str(&format!(" {}\n", a[*a_idx]));
                    }
                },
                Edit::Delete(a_idx) => {
                    if *a_idx < a.len() {
                        result.push_str(&format!("-{}\n", a[*a_idx]));
                    }
                },
                Edit::Insert(b_idx) => {
                    if *b_idx < b.len() {
                        result.push_str(&format!("+{}\n", b[*b_idx]));
                    }
                }
            }
        }
    }
    
    result
}

/// Calculate the range information for a hunk header
fn calculate_hunk_range(a: &[String], b: &[String], edits: &[Edit], hunk: &[usize]) -> (usize, usize, usize, usize) {
    // Default values to handle empty collections
    if hunk.is_empty() {
        return (0, 0, 0, 0);
    }
    
    // Initialize with safe values
    let mut a_start = usize::MAX;
    let mut a_end = 0;
    let mut b_start = usize::MAX;
    let mut b_end = 0;
    
    // Keep track of whether we've found any valid references
    let mut found_a = false;
    let mut found_b = false;
    
    for &edit_idx in hunk {
        if edit_idx >= edits.len() {
            continue;  // Skip invalid indices
        }
        
        match edits[edit_idx] {
            Edit::Equal(a_idx, b_idx) => {
                if a_idx < a.len() {
                    a_start = a_start.min(a_idx);
                    a_end = a_end.max(a_idx + 1);
                    found_a = true;
                }
                if b_idx < b.len() {
                    b_start = b_start.min(b_idx);
                    b_end = b_end.max(b_idx + 1);
                    found_b = true;
                }
            },
            Edit::Delete(a_idx) => {
                if a_idx < a.len() {
                    a_start = a_start.min(a_idx);
                    a_end = a_end.max(a_idx + 1);
                    found_a = true;
                }
            },
            Edit::Insert(b_idx) => {
                if b_idx < b.len() {
                    b_start = b_start.min(b_idx);
                    b_end = b_end.max(b_idx + 1);
                    found_b = true;
                }
            }
        }
    }
    
    // If we didn't find any valid a or b indices, use safe defaults
    if !found_a {
        a_start = 0;
        a_end = 0;
    }
    
    if !found_b {
        b_start = 0;
        b_end = 0;
    }
    
    // Calculate the count (always at least 0)
    let a_count = if a_end > a_start { a_end - a_start } else { 0 };
    let b_count = if b_end > b_start { b_end - b_start } else { 0 };
    
    (a_start, a_count, b_start, b_count)
}