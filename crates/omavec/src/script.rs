//! `OMAVEC_SCRIPT`: steps for the app to take once it has started, to
//! reproduce a bug or drive the installed binary from a terminal.
//!
//! ```text
//! OMAVEC_SCRIPT="Frame 0 0 400 300,Rectangle 20 20 100 80,Group,Export dist,Quit" omavec
//! ```
//!
//! Steps are separated by commas and taken one a frame:
//!
//! - a command by its name in `Command` (`Undo`, `Group`, `ZoomToFit`…);
//! - a tool and where to drag it, on the page: `Rectangle x y width
//!   height`, likewise `Frame`, `Ellipse`, `Polygon` and `Star`, and `Line
//!   x1 y1 x2 y2` and `Arrow` from one point to another;
//! - `Click x y` and `Drag x1 y1 x2 y2` with whichever tool is picked;
//! - `Open path`, `Save path` and `Export folder`.

use std::path::PathBuf;

use omavec_geom::kurbo::Point;

use crate::commands::Command;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Run(Command),
    /// Picks the tool the command picks, then drags from one point to the other.
    Draw(Command, Point, Point),
    Click(Point),
    Drag(Point, Point),
    Open(PathBuf),
    Save(PathBuf),
    Export(PathBuf),
}

/// The steps `text` spells out, or what is wrong with the first that isn't one.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    text.split(',').map(str::trim).filter(|step| !step.is_empty()).map(step).collect()
}

fn step(text: &str) -> Result<Step, String> {
    let (name, rest) = text.split_once(char::is_whitespace).map_or((text, ""), |(name, rest)| (name, rest.trim()));
    let command = |name: &str| Command::ALL.into_iter().find(|command| format!("{command:?}") == name);
    let numbers = || -> Result<[f64; 4], String> {
        let numbers: Vec<f64> = rest.split_whitespace().map(|number| number.parse().map_err(|_| format!("\"{text}\": {number} is not a number"))).collect::<Result<_, _>>()?;
        numbers.try_into().map_err(|_| format!("\"{text}\" takes four numbers"))
    };
    let path = || if rest.is_empty() { Err(format!("\"{text}\" needs a path")) } else { Ok(PathBuf::from(rest)) };
    match (name, command(name), command(&format!("{name}Tool"))) {
        ("Open", ..) if !rest.is_empty() => Ok(Step::Open(path()?)),
        ("Save", ..) if !rest.is_empty() => Ok(Step::Save(path()?)),
        ("Export", ..) if !rest.is_empty() => Ok(Step::Export(path()?)),
        ("Click", ..) => match rest.split_whitespace().map(str::parse).collect::<Result<Vec<f64>, _>>().as_deref() {
            Ok([x, y]) => Ok(Step::Click(Point::new(*x, *y))),
            _ => Err(format!("\"{text}\" takes two numbers")),
        },
        ("Drag", ..) => numbers().map(|[x1, y1, x2, y2]| Step::Drag(Point::new(x1, y1), Point::new(x2, y2))),
        // A line goes from one point to another; a box has a corner and a size.
        ("Line" | "Arrow", _, Some(tool)) => numbers().map(|[x1, y1, x2, y2]| Step::Draw(tool, Point::new(x1, y1), Point::new(x2, y2))),
        (_, _, Some(tool)) if !rest.is_empty() => numbers().map(|[x, y, width, height]| Step::Draw(tool, Point::new(x, y), Point::new(x + width, y + height))),
        (_, Some(command), _) if rest.is_empty() => Ok(Step::Run(command)),
        _ => Err(format!("\"{text}\" is not a step")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_is_steps_between_commas() {
        let steps = parse("Frame 0 0 400 300, Rectangle 20 20 100 80 ,Line 0 0 30 40,Click 5 6,Drag 1 2 3 4,Group,Undo,Save /tmp/a b.omavec,Export dist,Open x.omavecz,,Quit").unwrap();
        let point = |x: f64, y: f64| Point::new(x, y);
        assert_eq!(
            steps,
            [
                Step::Draw(Command::FrameTool, point(0.0, 0.0), point(400.0, 300.0)),
                Step::Draw(Command::RectangleTool, point(20.0, 20.0), point(120.0, 100.0)),
                Step::Draw(Command::LineTool, point(0.0, 0.0), point(30.0, 40.0)),
                Step::Click(point(5.0, 6.0)),
                Step::Drag(point(1.0, 2.0), point(3.0, 4.0)),
                Step::Run(Command::Group),
                Step::Run(Command::Undo),
                Step::Save("/tmp/a b.omavec".into()),
                Step::Export("dist".into()),
                Step::Open("x.omavecz".into()),
                Step::Run(Command::Quit),
            ]
        );
        // The commands that share a name with a step that takes a path.
        assert_eq!(parse("Save,Open,Export").unwrap(), [Step::Run(Command::Save), Step::Run(Command::Open), Step::Run(Command::Export)]);
        // A tool with nowhere to draw is the command that picks it.
        assert_eq!(parse("RectangleTool").unwrap(), [Step::Run(Command::RectangleTool)]);
        assert_eq!(parse("").unwrap(), []);
    }

    #[test]
    fn what_is_not_a_step_says_which_and_why() {
        assert_eq!(parse("Undo,Explode"), Err("\"Explode\" is not a step".into()));
        assert_eq!(parse("Rectangle 1 2 3"), Err("\"Rectangle 1 2 3\" takes four numbers".into()));
        assert_eq!(parse("Ellipse 1 2 3 wide"), Err("\"Ellipse 1 2 3 wide\": wide is not a number".into()));
        assert_eq!(parse("Click 1"), Err("\"Click 1\" takes two numbers".into()));
        assert_eq!(parse("Undo twice"), Err("\"Undo twice\" is not a step".into()));
        assert_eq!(parse("Rectangle"), Err("\"Rectangle\" is not a step".into()));
    }
}
