use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldChoice {
    Leave,
    Append,
    Overwrite,
}

pub fn detect_and_prompt() -> Option<ScaffoldChoice> {
    let files = scaffold_files();
    if files.iter().all(|p| p.exists()) {
        return None;
    }
    Some(ScaffoldChoice::Leave)
}

pub fn apply_scaffold(choice: ScaffoldChoice) -> io::Result<()> {
    let files = scaffold_files();
    for path in files {
        let exists = path.exists();
        match choice {
            ScaffoldChoice::Leave => {
                if !exists {
                    // do nothing
                }
            }
            ScaffoldChoice::Append => {
                ensure_parent(&path)?;
                if exists {
                    let mut file = fs::OpenOptions::new().append(true).open(&path)?;
                    writeln!(file, "\n\n{}", scaffold_template_for(&path))?;
                } else {
                    fs::write(&path, scaffold_template_for(&path))?;
                }
            }
            ScaffoldChoice::Overwrite => {
                ensure_parent(&path)?;
                fs::write(&path, scaffold_template_for(&path))?;
            }
        }
    }
    Ok(())
}

fn scaffold_files() -> Vec<PathBuf> {
    vec![
        PathBuf::from("README.md"),
        PathBuf::from(".claude/CLAUDE.md"),
        PathBuf::from("skills/README.md"),
    ]
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn scaffold_template_for(path: &Path) -> String {
    let p = path.to_string_lossy();
    if p.ends_with("README.md") {
        "# Project\n\nDescribe the project, its goals, and how to run it.\n".to_string()
    } else if p.ends_with(".claude/CLAUDE.md") {
        "# CLAUDE.md\n\nInstructions for agents working in this repo.\n".to_string()
    } else if p.ends_with("skills/README.md") {
        "# Skills\n\nList available skills and how to use them.\n".to_string()
    } else {
        "".to_string()
    }
}
