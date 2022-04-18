use clap::{Arg, ArgMatches, Command};
use mdbook::book::{Book, Chapter};
use mdbook::errors::Error;
use mdbook::preprocess::{CmdPreprocessor, Preprocessor, PreprocessorContext};
use pulldown_cmark::{CowStr, Event, Parser, Tag};
use std::io;
use std::process;

use tailwind_css::TailwindBuilder;

#[derive(Default)]
pub struct Tailwindcss;

impl Tailwindcss {
    pub fn new() -> Tailwindcss {
        Tailwindcss
    }
}

impl Preprocessor for Tailwindcss {
    fn name(&self) -> &str {
        "tailwindcss"
    }
    fn run(&self, _ctx: &PreprocessorContext, mut book: Book) -> Result<Book, Error> {
        book.for_each_mut(|book| {
            if let mdbook::BookItem::Chapter(chapter) = book {
                if let Err(e) = process_tailwindcss(chapter) {
                    eprintln!("tailwindcss error: {:?}", e);
                }
            }
        });
        Ok(book)
    }
    fn supports_renderer(&self, renderer: &str) -> bool {
        renderer == "html"
    }
}

struct ClassAnnotation {
    pub class: String,
    pub style: String,
    pub paragraph_start: usize,
    pub paragraph_end: Option<usize>,
}

/// This is where the markdown transformation actually happens.
/// Take paragraphs beginning with `{:.class-name}` and give them special rendering.
/// Mutation: the payload here is that it edits chapter.content.
fn process_tailwindcss(chapter: &mut Chapter) -> Result<(), Error> {
    // 1. Parse the inbound markdown into an Event vector.
    let incoming_events: Vec<Event> = Parser::new(&chapter.content).collect();

    let mut tailwind = TailwindBuilder::default();

    // 2. Find paragraphs beginning with the class annotator `{:.class-name}` and record their information in
    // a vector of ClassAnnotation structs.
    let mut class_annotations: Vec<ClassAnnotation> = vec![];
    for i in 0..incoming_events.len() {
        let event = &incoming_events[i];
        match *event {
            Event::Text(CowStr::Borrowed(text)) => {
                if i > 0 {
                    if let Event::Start(Tag::Paragraph) = incoming_events[i - 1] {
                        let v: Vec<_> = text.split("").collect();
                        let len_v = v.len();
                        if v[..4].join("") == "{:." && v[(len_v - 2)..].join("") == "}" {
                            let class = v[4..(len_v - 2)].join("").replace('.', "");

                            let mut c = vec![];
                            let mut s = vec![];

                            for kls in class.split(' ') {
                                match tailwind.inline(kls) {
                                    Ok(r) => s.push(r.1),
                                    Err(_) => c.push(kls),
                                }
                            }
                            class_annotations.push(ClassAnnotation {
                                class: c.join(" "),
                                style: s.join(""),
                                paragraph_start: i - 1,
                                paragraph_end: None,
                            })
                        }
                    }
                }
            }
            Event::End(Tag::Paragraph) => {
                let last = class_annotations.last_mut();
                if let Some(class_command) = last {
                    if class_command.paragraph_end.is_none() {
                        class_command.paragraph_end = Some(i);
                    }
                }
            }
            _ => {}
        }
    }

    // 3. Construct a new_events vector with <div class="class-name">\n \n</div> around the annotated paragraphs
    // (and with the class annotation removed).
    let mut slices = vec![];
    let mut last_end = 0;
    let div_starts: Vec<Event> = class_annotations
        .iter()
        //.map(|ca| Event::Html(CowStr::from(format!("<div class=\"{}\">", ca.class))))
        .map(|ca| {
            Event::Html(CowStr::from(format!(
                "<div class\"{}\" style=\"{}\">",
                ca.class, ca.style
            )))
        })
        .collect();
    let div_end: Vec<Event> = vec![Event::Html(CowStr::from("</div>"))];
    //let div_end: Vec<Event> = vec![Event::Html(CowStr::from("end"))];
    for (i, ca) in class_annotations.iter().enumerate() {
        // Add unclassed events.
        slices.push(&incoming_events[last_end..ca.paragraph_start]);

        last_end = ca.paragraph_end.unwrap_or(incoming_events.len() - 1);

        let paragraph = &incoming_events[ca.paragraph_start..(last_end + 1)];

        // Add <div class="class-name">
        slices.push(&div_starts[i..i + 1]);

        // Add paragraph opener.
        slices.push(&paragraph[0..1]);

        // Add the rest of the paragraph, skipping the class annotation.
        slices.push(&paragraph[2..]);

        // Add </div>.
        slices.push(&div_end[..]);
    }
    slices.push(&incoming_events[last_end..]);
    let new_events = slices.concat();

    // 4. Update chapter.content using markdown generated from the new event vector.
    let mut buf = String::with_capacity(chapter.content.len() + 128);
    pulldown_cmark_to_cmark::cmark(new_events.into_iter(), &mut buf).expect("can re-render cmark");
    chapter.content = buf;
    Ok(())
}

/// Housekeeping:
/// 1. Check compatibility between preprocessor and mdbook
/// 2. deserialize, run the transformation, and reserialize.
fn handle_preprocessing(pre: &dyn Preprocessor) -> Result<(), Error> {
    let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;

    if ctx.mdbook_version != mdbook::MDBOOK_VERSION {
        // We should probably use the `semver` crate to check compatibility
        // here...
        eprintln!(
            "Warning: The {} plugin was built against version {} of mdbook, \
             but we're being called from version {}",
            pre.name(),
            mdbook::MDBOOK_VERSION,
            ctx.mdbook_version
        );
    }

    let processed_book = pre.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed_book)?;

    Ok(())
}

/// Check to see if we support the processor (mdbook-tailwindcss only supports html right now)
fn handle_supports(pre: &dyn Preprocessor, sub_args: &ArgMatches) -> ! {
    let renderer = sub_args.value_of("renderer").expect("Required argument");
    let supported = pre.supports_renderer(&renderer);

    if supported {
        process::exit(0);
    } else {
        process::exit(1);
    }
}

fn main() {
    // 1. Define command interface, requiring renderer to be specified.
    let matches = Command::new("mdbook-tailwindcss")
        .author("Jeffrey Jose, jeffjosejeff@gmail.com")
        .version("0.0.1")
        .about("An mdbook preprocessor for tailwindcss classes")
        .subcommand(
            Command::new("supports")
                .arg(Arg::new("renderer").required(true))
                .about("Checks whether a renderer is supported by this preprocessor"),
        )
        .get_matches();

    // 2. Instantiate the preprocessor.
    let preprocessor = Tailwindcss::new();

    if let Some(sub_args) = matches.subcommand_matches("supports") {
        handle_supports(&preprocessor, sub_args);
    } else if let Err(e) = handle_preprocessing(&preprocessor) {
        eprintln!("{}", e);
        process::exit(1);
    }
}
