use anyhow::{Result, bail, Context};

#[derive(Debug, Default)]
pub struct Choices {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Clone)]
pub enum Choice {
    Plus (f64, String),
    Minus(f64, String),
    Set  (f64, String),
    Comment(String),
}

#[derive(Default)]
pub struct ChoiceSelections {
    selections: Vec<ChoiceSelection>,
    cursor:     usize,
}

pub struct ChoiceSelection {
    choice:     Choice,
    selected:   bool,
    real_index: usize,
}

impl ChoiceSelections {
    pub fn new(choices: &Choices) -> Self {
        Self {
            selections: choices.choices.iter()
                .enumerate()
                .filter_map(|(index, choice)|
                    match choice {
                        Choice::Plus(_, _) | Choice::Minus(_, _) | Choice::Set(_, _) => {
                            Some(ChoiceSelection {
                                choice:     choice.clone(),
                                selected:   false,
                                real_index: index,
                            })
                        }
                        Choice::Comment(_) => None,
                    }
                )
                .collect(),
            cursor: 0,
        }
    }

    pub fn selections(&self) -> &[ChoiceSelection] {
        &self.selections
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn real_cursor(&self) -> usize {
        self.selections[self.cursor].real_index
    }
    
    pub fn toggle_selection(&mut self) {
        let selection = &mut self.selections[self.cursor];

        match selection.choice() {
            Choice::Plus(_, _) | Choice::Minus(_, _) => {
                for other in &mut self.selections {
                    if matches!(other.choice(), Choice::Set(_, _)) {
                        other.selected = false;
                    }
                }

                self.selections[self.cursor].selected = !self.selections[self.cursor].selected;
            }
            Choice::Set(_, _)  => {
                for other in &mut self.selections {
                    other.selected = false;
                }

                self.selections[self.cursor].selected = !self.selections[self.cursor].selected;
            }
            Choice::Comment(_) => unreachable!(),
        }
    }

    pub fn cursor_next(&mut self) {
        self.cursor = (self.cursor + 1) % self.selections.len();
    }

    pub fn cursor_prev(&mut self) {
        self.cursor = (self.cursor + self.selections.len() - 1) % self.selections.len();
    }

    pub fn try_cursor_set(&mut self, new_cursor: usize) -> bool {
        if new_cursor < self.selections.len() {
            self.cursor = new_cursor;
            true
        } else {
            false
        }
    }

    pub fn from_real_index(&self, real_index: usize) -> Option<(usize, &ChoiceSelection)> {
        self.selections.iter()
            .enumerate()
            .find(|(_, selection)| selection.real_index == real_index)
    }
}

impl ChoiceSelection {
    pub fn choice(&self) -> &Choice {
        &self.choice
    }

    pub fn selected(&self) -> bool {
        self.selected
    }

    pub fn toggle(&mut self) {
        self.selected = !self.selected;
    }

    pub fn real_index(&self) -> usize {
        self.real_index
    }
}

pub fn parse_choices(contents: &str) -> Result<Choices> {
    let mut choices = vec![];
    
    for (line_index, line) in contents.lines().enumerate() {
        let line = line.trim();
        let line_number = line_index + 1;
        
        let (first_char, second_char) = match <[char; 2]>::try_from(line.chars().take(2).collect::<Vec<char>>()) {
            Ok([first_char, second_char]) => (first_char, second_char),
            Err(_) => {
                // Not a semantic line -- leave it as a comment
                choices.push(Choice::Comment(line.to_string()));
                continue;
            }
        };

        let fallible = || {
            let choice = match (first_char, second_char) {
                ('+', '0'..='9' | '.') => {
                    let (number, rest) = parse_number(skip_first_char(line))?;
    
                    Choice::Plus(number, rest.to_string())
                }
                ('-', '0'..='9' | '.') => {
                    let (number, rest) = parse_number(skip_first_char(line))?;
    
                    Choice::Minus(number, rest.to_string())
                }
                ('=', '0'..='9' | '.') => {
                    let (number, rest) = parse_number(skip_first_char(line))?;
    
                    Choice::Set(number, rest.to_string())
                }
                ('0'..='9', _) | ('.', '0'..='9') => {
                    bail!("Choice file should never start with a number\n\
                           If you meant to add to the mark, use +number\n\
                           If you meant to set the mark, use =number\n\
                           If you didn't mean either of these, you're bound to confuse markers");
                }
                _ => {
                    Choice::Comment(line.to_string())
                }
            };

            Ok(choice)
        };

        let choice = fallible()
            .with_context(|| format!("Choice file error on line {line_number}"))?;

        choices.push(choice);
    }

    Ok(Choices { choices })
}

fn skip_first_char(line: &str) -> &str {
    match line.char_indices().skip(1).next() {
        Some((index, _)) => &line[index..],
        None => line,
    }
}

fn parse_number(line: &str) -> Result<(f64, &str)> {
    let termination = line.char_indices()
        .find(|char| !matches!(char.1, '0'..='9' | '.'));

    let (number_part, rest) = match termination {
        Some((index, _)) => (&line[..index], line[index..].trim()),
        None => (line, ""),
    };

    let number = number_part.parse()?;

    Ok((number, rest))
}
