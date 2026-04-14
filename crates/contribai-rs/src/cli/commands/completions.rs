//! Handles `Commands::Completions` — generate shell completions.

use clap::CommandFactory;
use clap_complete::{generate, Shell};
use console::style;
use std::io;

pub fn run_completions(shell: Shell) -> anyhow::Result<()> {
    println!("{}", style("🔧 Generating Shell Completions").cyan().bold());
    println!("{}", style("━".repeat(50)).dim());
    println!();

    let mut cmd = crate::cli::Cli::command();
    let bin_name = "contribai";

    generate(shell, &mut cmd, bin_name, &mut io::stdout());

    println!();
    match shell {
        Shell::Bash => {
            println!(
                "  {} Save to: ~/.local/share/bash-completion/completions/{}",
                style("💡").bold(),
                bin_name
            );
            println!("  {} Or: /etc/bash_completion.d/", style(" ").dim());
        }
        Shell::Zsh => {
            println!("  {} Save to: ~/.zfunc/_{}", style("💡").bold(), bin_name);
            println!(
                "  {} Then add 'fpath=(~/.zfunc $fpath)' to ~/.zshrc",
                style(" ").dim()
            );
        }
        Shell::Fish => {
            println!(
                "  {} Save to: ~/.config/fish/completions/{}.fish",
                style("💡").bold(),
                bin_name
            );
        }
        Shell::PowerShell => {
            println!(
                "  {} Save to: ~/.config/powershell/completions/{}.ps1",
                style("💡").bold(),
                bin_name
            );
        }
        Shell::Elvish => {
            println!(
                "  {} Save to: ~/.elvish/lib/{}.elv",
                style("💡").bold(),
                bin_name
            );
        }
        _ => {}
    }
    println!();

    Ok(())
}
