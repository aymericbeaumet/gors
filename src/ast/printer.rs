use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;

pub struct Printer<W: Write> {
    pub w: W,
    line: usize,
    depth: usize,
    lines: HashMap<u64, usize>,
}

impl<W: Write> Printer<W> {
    pub fn new(w: W) -> Self {
        Printer {
            w,
            line: 0,
            depth: 0,
            lines: HashMap::default(),
        }
    }

    pub fn prefix(&mut self) -> std::io::Result<()> {
        write!(self.w, "{:6}  ", self.line)?;
        for _ in 0..self.depth {
            self.write(".  ")?;
        }
        Ok(())
    }

    pub fn open_bracket(&mut self) -> std::io::Result<()> {
        self.depth += 1;
        self.write("{")?;
        self.newline()
    }

    pub fn close_bracket(&mut self) -> std::io::Result<()> {
        self.depth -= 1;
        self.prefix()?;
        self.write("}")?;
        self.newline()
    }

    pub fn newline(&mut self) -> std::io::Result<()> {
        self.line += 1;
        self.write("\n")
    }

    pub fn write(&mut self, buf: &str) -> std::io::Result<()> {
        self.w.write_all(buf.as_bytes())
    }

    pub fn prevent_circular<T: Hash>(
        &mut self,
        val: T,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // requirement: this hash function should not produce collisions
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        val.hash(&mut hasher);
        let h = hasher.finish();

        if let Some(line) = self.lines.get(&h) {
            write!(self.w, "*(obj @ {})", line)?;
            self.newline()?;
            Ok(true)
        } else {
            self.lines.insert(h, self.line);
            Ok(false)
        }
    }
}

pub type PrintResult = Result<(), Box<dyn std::error::Error>>;

pub trait Printable<W: Write> {
    fn print(&self, _: &mut Printer<W>) -> PrintResult;
}
