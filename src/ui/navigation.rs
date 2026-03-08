use ratatui::widgets::ListState;

pub struct NavigatableList {
    pub state: ListState,
    pub options: Vec<String>
}


impl NavigatableList {
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i>=self.options.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.options.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0
        };
        self.state.select(Some(i));
    }
}