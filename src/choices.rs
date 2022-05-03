use anyhow::Result;

#[derive(Debug)]
pub struct Choices {
    pub choices: Vec<Choice>,
}

#[derive(Debug)]
pub enum Choice {
    Plus (u32, String),
    Minus(u32, String),
    Set  (u32, String),
    Comment(String),
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
