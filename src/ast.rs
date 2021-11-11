pub enum Node {
    File(File),
}

pub struct File {}

pub fn fprint<W: std::io::Write>(w: &mut W, node: &Node) -> Result<(), Box<dyn std::error::Error>> {
    use Node::*;

    match node {
        File(_) => write!(w, "File")?,
    }

    Ok(())
}
