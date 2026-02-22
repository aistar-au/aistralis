use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub buffer: String,
    pub cursor: usize,
}

#[derive(Default, Debug)]
pub struct InputState {
    pub buffer: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub history_stash: Option<EditorSnapshot>,
    pub undo_stack: Vec<EditorSnapshot>,
    pub redo_stack: Vec<EditorSnapshot>,
}

pub struct InputEditor {
    pub input_state: InputState,
}

pub enum InputAction {
    None,
    Submit(String),
    Interrupt,
    Quit,
}

impl InputEditor {
    pub fn new() -> Self {
        Self {
            input_state: InputState::default(),
        }
    }

    pub fn buffer(&self) -> &str {
        &self.input_state.buffer
    }

    pub fn cursor(&self) -> usize {
        self.input_state.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.input_state.buffer.is_empty()
    }

    pub fn clamp_cursor_to_boundary_left(&self, mut idx: usize) -> usize {
        idx = idx.min(self.input_state.buffer.len());
        while idx > 0 && !self.input_state.buffer.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    pub fn prev_char_boundary(&self, idx: usize) -> usize {
        let i = self.clamp_cursor_to_boundary_left(idx);
        if i == 0 {
            return 0;
        }
        let mut j = i - 1;
        while j > 0 && !self.input_state.buffer.is_char_boundary(j) {
            j -= 1;
        }
        j
    }

    pub fn next_char_boundary(&self, idx: usize) -> usize {
        let i = self.clamp_cursor_to_boundary_left(idx);
        if i >= self.input_state.buffer.len() {
            return self.input_state.buffer.len();
        }
        match self.input_state.buffer[i..].chars().next() {
            Some(ch) => i + ch.len_utf8(),
            None => self.input_state.buffer.len(),
        }
    }

    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            buffer: self.input_state.buffer.clone(),
            cursor: self.input_state.cursor,
        }
    }

    pub fn push_undo(&mut self) {
        self.input_state.undo_stack.push(self.snapshot());
        self.input_state.redo_stack.clear();
    }

    pub fn restore(&mut self, snap: EditorSnapshot) {
        self.input_state.buffer = snap.buffer;
        self.input_state.cursor = self.clamp_cursor_to_boundary_left(snap.cursor);
    }

    pub fn insert_str(&mut self, value: &str) {
        self.input_state.history_index = None;
        self.input_state.history_stash = None;
        let cursor = self.clamp_cursor_to_boundary_left(self.input_state.cursor);
        self.push_undo();
        self.input_state.buffer.insert_str(cursor, value);
        self.input_state.cursor = cursor + value.len();
    }

    pub fn backspace(&mut self) {
        let end = self.clamp_cursor_to_boundary_left(self.input_state.cursor);
        if end == 0 {
            return;
        }
        self.input_state.history_index = None;
        self.input_state.history_stash = None;
        let start = self.prev_char_boundary(end);
        self.push_undo();
        self.input_state.buffer.replace_range(start..end, "");
        self.input_state.cursor = start;
    }

    pub fn delete(&mut self) {
        let start = self.clamp_cursor_to_boundary_left(self.input_state.cursor);
        if start >= self.input_state.buffer.len() {
            return;
        }
        self.input_state.history_index = None;
        self.input_state.history_stash = None;
        let end = self.next_char_boundary(start);
        self.push_undo();
        self.input_state.buffer.replace_range(start..end, "");
        self.input_state.cursor = start;
    }

    pub fn submit(&mut self) -> Option<String> {
        let value = self
            .input_state
            .buffer
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string();
        if value.is_empty() {
            return None;
        }
        self.input_state
            .history
            .push(self.input_state.buffer.clone());
        self.input_state.history_index = None;
        self.input_state.history_stash = None;
        self.push_undo();
        self.input_state.buffer.clear();
        self.input_state.cursor = 0;
        Some(value)
    }

    pub fn history_up(&mut self) {
        if self.input_state.history.is_empty() {
            return;
        }

        if self.input_state.history_index.is_none() {
            self.input_state.history_stash = Some(self.snapshot());
        }
        let next_index = match self.input_state.history_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => 0,
            None => self.input_state.history.len().saturating_sub(1),
        };
        self.input_state.history_index = Some(next_index);
        self.input_state.buffer = self.input_state.history[next_index].clone();
        self.input_state.cursor = self.input_state.buffer.len();
    }

    pub fn history_down(&mut self) {
        let Some(idx) = self.input_state.history_index else {
            return;
        };

        if idx + 1 >= self.input_state.history.len() {
            self.input_state.history_index = None;
            if let Some(stash) = self.input_state.history_stash.take() {
                self.restore(stash);
            } else {
                self.input_state.buffer.clear();
                self.input_state.cursor = 0;
            }
        } else {
            let next = idx + 1;
            self.input_state.history_index = Some(next);
            self.input_state.buffer = self.input_state.history[next].clone();
            self.input_state.cursor = self.input_state.buffer.len();
        }
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.input_state.undo_stack.pop() {
            self.input_state.redo_stack.push(self.snapshot());
            self.restore(previous);
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.input_state.redo_stack.pop() {
            self.input_state.undo_stack.push(self.snapshot());
            self.restore(next);
        }
    }

    pub fn apply_event(&mut self, event: Event) -> InputAction {
        match event {
            Event::Paste(text) => {
                self.insert_str(&text);
                InputAction::None
            }
            Event::Key(key) => self.apply_key(key),
            _ => InputAction::None,
        }
    }

    pub fn apply_key(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.input_state.buffer.is_empty() {
                    return InputAction::Quit;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return InputAction::Interrupt;
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_str("\n");
            }
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.undo();
            }
            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.redo();
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_str("\n");
            }
            KeyCode::Enter => {
                if let Some(value) = self.submit() {
                    return InputAction::Submit(value);
                }
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => {
                self.input_state.cursor = self.prev_char_boundary(self.input_state.cursor);
            }
            KeyCode::Right => {
                self.input_state.cursor = self.next_char_boundary(self.input_state.cursor);
            }
            KeyCode::Home => self.input_state.cursor = 0,
            KeyCode::End => self.input_state.cursor = self.input_state.buffer.len(),
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            KeyCode::Char(ch) => self.insert_str(&ch.to_string()),
            KeyCode::Esc => {
                if self.input_state.buffer.is_empty() {
                    return InputAction::Submit("esc".to_string());
                }
            }
            _ => {}
        }

        InputAction::None
    }
}

impl Default for InputEditor {
    fn default() -> Self {
        Self::new()
    }
}
