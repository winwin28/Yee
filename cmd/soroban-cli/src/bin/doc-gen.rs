use std::{
    env,
    path::{Path, PathBuf},
};

type DynError = Box<dyn std::error::Error>;

fn main() -> Result<(), DynError> {
    doc_gen()?;
    Ok(())
}

fn doc_gen() -> std::io::Result<()> {
    let out_dir = project_root();
    let options = clap_markdown::MarkdownOptions::new()
        .show_footer(false)
        .show_table_of_contents(false)
        .title("Stellar CLI Manual".to_string());

    let content = generate_markdown_with_aliases::<soroban_cli::Root>(&options);

    std::fs::write(out_dir.join("FULL_HELP_DOCS.md"), content)?;

    Ok(())
}

fn generate_markdown_with_aliases<C: clap::CommandFactory>(
    options: &clap_markdown::MarkdownOptions,
) -> String {
    let command = C::command();
    let mut markdown_content = clap_markdown::help_markdown_custom::<C>(options);
    add_aliases_recursively(&command, &mut markdown_content, 2);
    markdown_content
}

fn add_aliases_recursively(command: &clap::Command, content: &mut String, level: usize) {

    for arg in command.get_arguments() {

        let arg_name = format!("--{}", arg.get_id().as_str().to_kebab_case());
        let mut alias_list = vec![];

        let visible_aliases: Vec<String> = arg
            .get_visible_aliases()
            .into_iter()
            .flatten()
            .map(|alias| format!("--{}", alias.to_kebab_case()))
            .collect();

            alias_list.push(arg_name.clone());
        alias_list.extend(visible_aliases);

        content.push_str(&format!(
            "\n\n**Argument {}**: {}\n",
            arg_name,
            alias_list.join(", ")
        ));
    }

    // Recursively process subcommands
    for subcommand in command.get_subcommands() {
        add_aliases_recursively(subcommand, content, level + 1);
    }
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

trait KebabCase {
    fn to_kebab_case(&self) -> String;
}

impl KebabCase for str {
    fn to_kebab_case(&self) -> String {
        self.replace('_', "-")
    }
}

impl KebabCase for clap::Id {
    fn to_kebab_case(&self) -> String {
        self.as_str().to_kebab_case()
    }
}
