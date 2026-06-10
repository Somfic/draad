//! Small indent-aware string builder the emitters write into. Replaces
//! the manual `out.push_str(&format!("{indent}{line}\n"))` ritual; the
//! emitters no longer carry the indent prefix as a parameter.

/// Append-only builder. `line` writes the current indent + the given
/// content + `\n`. `indented` runs a closure with the indent level
/// bumped by one. `raw` appends verbatim with no indent or trailing
/// newline (used for the prebuilt TS runtime block).
pub(super) struct Writer {
    out: String,
    indent_unit: &'static str,
    level: usize,
}

impl Writer {
    pub(super) fn new(indent_unit: &'static str) -> Self {
        Self {
            out: String::new(),
            indent_unit,
            level: 0,
        }
    }

    /// Indent + content + `\n`. Empty `content` still indents, which is
    /// usually not what you want — use [`blank`](Self::blank) for that.
    pub(super) fn line(&mut self, content: &str) {
        for _ in 0..self.level {
            self.out.push_str(self.indent_unit);
        }
        self.out.push_str(content);
        self.out.push('\n');
    }

    /// A single `\n` with no indent.
    pub(super) fn blank(&mut self) {
        self.out.push('\n');
    }

    /// Append `content` verbatim — no indent, no newline. Used to splice
    /// in a multi-line block that already carries its own formatting
    /// (e.g. the embedded TS runtime).
    pub(super) fn raw(&mut self, content: &str) {
        self.out.push_str(content);
    }

    /// Run `body` with the indent level temporarily bumped by one.
    pub(super) fn indented<F: FnOnce(&mut Self)>(&mut self, body: F) {
        self.level += 1;
        body(self);
        self.level -= 1;
    }

    pub(super) fn into_string(self) -> String {
        self.out
    }
}
