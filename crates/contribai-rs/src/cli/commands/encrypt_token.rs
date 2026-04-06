//! Handles `Commands::EncryptToken` — encrypt a GitHub token for secure storage.

use console::style;
use contribai::core::crypto::encrypt_token;
use dialoguer::Password;

pub fn run_encrypt_token(token: Option<&str>, passphrase: Option<&str>) -> anyhow::Result<()> {
    // Get token
    let token = match token {
        Some(t) => t.to_string(),
        None => Password::new()
            .with_prompt("GitHub token to encrypt")
            .interact()?,
    };

    // Get passphrase
    let passphrase = match passphrase {
        Some(p) => p.to_string(),
        None => Password::new()
            .with_prompt("Encryption passphrase")
            .with_confirmation("Confirm passphrase", "Passphrases don't match")
            .interact()?,
    };

    let encrypted = encrypt_token(&token, &passphrase)?;

    println!();
    println!(
        "{}",
        style("✅ Token encrypted successfully").green().bold()
    );
    println!();
    println!("Add this to your config.yaml:");
    println!();
    println!("  github:");
    println!("    token_encrypted: \"{}\"", encrypted);
    println!();
    println!(
        "{} Store CONTRIBUTAI_ENCRYPTION_KEY in your environment to decrypt at runtime",
        style("💡").bold()
    );
    println!();

    Ok(())
}
