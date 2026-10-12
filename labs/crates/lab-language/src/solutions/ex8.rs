pub fn enabled_features() -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg!(feature = "solution") {
        out.push("solution");
    }
    if cfg!(feature = "shout") {
        out.push("shout");
    }
    out
}

#[cfg(feature = "shout")]
pub fn shout(name: &str) -> String {
    format!("{}!", name.to_uppercase())
}
