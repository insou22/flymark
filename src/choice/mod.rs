use anyhow::Result;

#[derive(Debug, Default)]
pub struct Choices {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Clone)]
pub enum Choice {
    Plus (u32, String),
    Minus(u32, String),
    Set  (u32, String),
    Comment(String),
}

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

    pub fn from_real_index(&self, real_index: usize) -> Option<(usize, &ChoiceSelection)> {
        self.selections.iter()
            .enumerate()
            .find(|(_, selection)| selection.real_index == real_index)
    }
}

pub fn parse_choices(contents: &str) -> Result<Choices> {
    let mut choices = vec![];
    
    for line in contents.lines() {
        let line = line.trim();
        
        let first_char = match line.chars().next() {
            Some(first_char) => first_char,
            None => {
                // Empty line -- just a blank comment
                choices.push(Choice::Comment(String::new()));
                continue;
            }
        };

        let choice = match first_char {
            '+' => {
                let (number, rest) = parse_number(skip_first_char(line))?;

                Choice::Plus(number, rest.to_string())
            }
            '-' => {
                let (number, rest) = parse_number(skip_first_char(line))?;

                Choice::Minus(number, rest.to_string())
            }
            '0'..='9' => {
                let (number, rest) = parse_number(line)?;

                Choice::Set(number, rest.to_string())
            }
            _ => {
                Choice::Comment(line.to_string())
            }
        };

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

fn parse_number(line: &str) -> Result<(u32, &str)> {
    let termination = line.char_indices()
        .find(|char| !matches!(char.1, '0'..='9'));

    let (number_part, rest) = match termination {
        Some((index, _)) => (&line[..index], line[index..].trim()),
        None => (line, ""),
    };

    let number = number_part.parse::<u32>()?;

    Ok((number, rest))
}
