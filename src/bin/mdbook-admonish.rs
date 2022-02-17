use clap::{crate_version, Arg, ArgMatches, Command};
use mdbook::errors::Error;
use mdbook::preprocess::{CmdPreprocessor, Preprocessor};
use mdbook_admonish::Admonish;

use std::{io, process};

pub fn make_app() -> Command<'static> {
    let mut command = Command::new("mdbook-admonish")
        .version(crate_version!())
        .about("mdbook preprocessor to add support for admonitions");
    command = command.subcommand(
        Command::new("supports")
            .arg(Arg::new("renderer").required(true))
            .about("Check whether a renderer is supported by this preprocessor"),
    );

    #[cfg(feature = "cli-install")]
    {
        command = command.subcommand(
        Command::new("install")
            .arg(Arg::new("dir").default_value(".").help(
                "Root directory for the book,\nshould contain the configuration file (`book.toml`)",
            ))
            .about("Install the required assset files and include it in the config"));
    }
    command
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let matches = make_app().get_matches();

    if let Some(sub_args) = matches.subcommand_matches("supports") {
        handle_supports(sub_args);
    } else if let Some(sub_args) = matches.subcommand_matches("install") {
        #[cfg(feature = "cli-install")]
        {
            install::handle_install(sub_args);
        }
        #[cfg(not(feature = "cli-install"))]
        {
            panic!("cli-install feature not enabled: {:?}", sub_args)
        }
    } else if let Err(e) = handle_preprocessing() {
        eprintln!("{}", e);
        process::exit(1);
    }
}

fn handle_preprocessing() -> Result<(), Error> {
    let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;

    if ctx.mdbook_version != mdbook::MDBOOK_VERSION {
        eprintln!(
            "Warning: The mdbook-admonish preprocessor was built against version \
             {} of mdbook, but we're being called from version {}",
            mdbook::MDBOOK_VERSION,
            ctx.mdbook_version
        );
    }

    let processed_book = Admonish.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed_book)?;

    Ok(())
}

fn handle_supports(sub_args: &ArgMatches) -> ! {
    let renderer = sub_args.value_of("renderer").expect("Required argument");
    let supported = Admonish.supports_renderer(renderer);

    // Signal whether the renderer is supported by exiting with 1 or 0.
    if supported {
        process::exit(0);
    } else {
        process::exit(1);
    }
}

#[cfg(feature = "cli-install")]
mod install {
    use clap::ArgMatches;
    use std::{
        fs::{self, File},
        io::Write,
        path::PathBuf,
        process,
    };

    const ADMONISH_CSS: &[u8] = include_bytes!("assets/mdbook-admonish.css");
    const ADMONISH_FILES: &[(&str, &[u8])] = &[("mdbook-admonish.css", ADMONISH_CSS)];

    use toml_edit::{value, Array, Document, Item, Table, Value};
    pub fn handle_install(sub_args: &ArgMatches) -> () {
        let dir = sub_args.value_of("dir").expect("Required argument");
        let proj_dir = PathBuf::from(dir);
        let config = proj_dir.join("book.toml");

        if !config.exists() {
            log::error!("Configuration file '{}' missing", config.display());
            process::exit(1);
        }

        log::info!("Reading configuration file {}", config.display());
        let toml = fs::read_to_string(&config).expect("can't read configuration file");
        let mut doc = toml
            .parse::<Document>()
            .expect("configuration is not valid TOML");

        let has_pre = has_preprocessor(&mut doc);
        if !has_pre {
            log::info!("Adding preprocessor configuration");
            add_preprocessor(&mut doc);
        }

        let added_files = add_additional_files(&mut doc);

        if !has_pre || added_files {
            log::info!("Saving changed configuration to {}", config.display());
            let toml = doc.to_string();
            let mut file =
                File::create(config).expect("can't open configuration file for writing.");
            file.write_all(toml.as_bytes())
                .expect("can't write configuration");
        }

        let mut printed = false;
        for (name, content) in ADMONISH_FILES {
            let filepath = proj_dir.join(name);
            if !printed {
                printed = true;
                log::info!(
                    "Writing additional files to project directory at {}",
                    proj_dir.display()
                );
            }
            log::debug!("Writing content for '{}' into {}", name, filepath.display());
            let mut file = File::create(filepath).expect("can't open file for writing");
            file.write_all(content)
                .expect("can't write content to file");
        }

        log::info!("Files & configuration for mdbook-admonish are installed. You can start using it in your book.");
        let codeblock = r#"```admonish warning
A beautifully styled message.
```"#;
        log::info!("Add a code block like:\n{}", codeblock);

        process::exit(0);
    }

    fn add_additional_files(doc: &mut Document) -> bool {
        let mut changed = false;

        let file = "mdbook-admonish.css";
        let additional_js = additional(doc, "css");
        if has_file(&additional_js, file) {
            log::debug!("'{}' already in 'additional-js'. Skipping", file)
        } else {
            log::info!("Adding additional files to configuration");
            log::debug!("Adding '{}' to 'additional-js'", file);
            insert_additional(doc, "css", file);
            changed = true;
        }

        changed
    }

    fn additional<'a>(doc: &'a mut Document, additional_type: &str) -> Option<&'a mut Array> {
        let doc = doc.as_table_mut();

        let item = doc.get_mut("output")?;
        let item = item.as_table_mut()?.get_mut("html")?;
        let item = item
            .as_table_mut()?
            .get_mut(&format!("additional-{}", additional_type))?;
        item.as_array_mut()
    }

    fn has_preprocessor(doc: &mut Document) -> bool {
        doc.get("preprocessor")
            .and_then(|p| p.get("admonish"))
            .map(|m| matches!(m, Item::Table(_)))
            .unwrap_or(false)
    }

    fn add_preprocessor(doc: &mut Document) {
        let doc = doc.as_table_mut();

        let empty_table = Item::Table(Table::default());

        let item = doc.entry("preprocessor").or_insert(empty_table.clone());
        let item = item
            .as_table_mut()
            .unwrap()
            .entry("admonish")
            .or_insert(empty_table);
        item["command"] = value("mdbook-admonish");
    }

    fn has_file(elem: &Option<&mut Array>, file: &str) -> bool {
        match elem {
            Some(elem) => elem.iter().any(|elem| match elem.as_str() {
                None => true,
                Some(s) => s.ends_with(file),
            }),
            None => false,
        }
    }

    fn insert_additional(doc: &mut Document, additional_type: &str, file: &str) {
        let doc = doc.as_table_mut();

        let empty_table = Item::Table(Table::default());
        let empty_array = Item::Value(Value::Array(Array::default()));
        let item = doc.entry("output").or_insert(empty_table.clone());
        let item = item
            .as_table_mut()
            .unwrap()
            .entry("html")
            .or_insert(empty_table);
        let array = item
            .as_table_mut()
            .unwrap()
            .entry(&format!("additional-{}", additional_type))
            .or_insert(empty_array);
        let _ = array
            .as_value_mut()
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(file);
    }
}
