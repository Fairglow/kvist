use pulldown_cmark::{Event, Options, Parser};

fn main() {
    let md = "# Heading 1\n\nThis is paragraph one.\n\nThis is paragraph two with a\nsoft break in the middle.\n\n## Heading 2\n\n- Item one\n- Item two\n\n```hemlock\nlet x = 42;\nlet y = 100;\n```\n\n[Link](https://example.com)";
    
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(md, options);
    
    for event in parser {
        println!("{:?}", event);
    }
}