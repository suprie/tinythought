use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, SaltString};

fn main() -> anyhow::Result<()> {
    let password = rpassword::prompt_password("Admin password: ")?;
    if password.len() < 12 {
        anyhow::bail!("use a password of at least 12 characters");
    }
    let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>())
        .map_err(|error| anyhow::anyhow!("could not generate salt: {error}"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("could not hash password: {error}"))?;
    println!("{hash}");
    Ok(())
}
