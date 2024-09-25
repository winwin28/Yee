use std::{
    fs::{create_dir_all, metadata, write, Metadata},
    io,
    path::{Path, PathBuf},
    str,
};

use clap::Parser;
use rust_embed::RustEmbed;

use crate::{commands::global, deprecated_arg, print, utils};

#[derive(Parser, Debug, Clone)]
#[group(skip)]
pub struct Cmd {
    pub project_path: String,

    // TODO: remove in 23.0
    #[arg(
        short,
        long,
        hide = true,
        value_parser = deprecated_arg!(String, "Adding examples via cli is no longer supported. You can still clone examples from the repo https://github.com/stellar/soroban-examples")
    )]
    pub with_example: Option<String>,

    // TODO: remove in 23.0
    #[arg(
        long,
        hide = true,
        value_parser = deprecated_arg!(String, "Using frontend template via cli is no longer supported. You can search for frontend templates using github tags, such as `soroban-template` or `soroban-frontend-template`"),
    )]
    pub frontend_template: Option<String>,

    #[arg(long, long_help = "Overwrite all existing files.")]
    pub overwrite: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(String, io::Error),

    #[error(transparent)]
    Std(#[from] std::io::Error),

    #[error("failed to convert bytes to string: {0}")]
    ConvertBytesToString(#[from] str::Utf8Error),

    #[error("contract package already exists: {0}")]
    AlreadyExists(String),

    #[error("provided project path exists and is not a directory")]
    PathExistsNotDir,

    #[error("provided project path exists and is not a cargo workspace root directory. Hint: run init on an empty or non-existing directory"
    )]
    PathExistsNotCargoProject,
}

impl Cmd {
    #[allow(clippy::unused_self)]
    pub fn run(&self, global_args: &global::Args) -> Result<(), Error> {
        let runner = Runner {
            args: self.clone(),
            print: print::Print::new(global_args.quiet),
        };

        runner.run()
    }
}

#[derive(RustEmbed)]
#[folder = "src/utils/contract-init-template"]
struct TemplateFiles;

struct Runner {
    args: Cmd,
    print: print::Print,
}

impl Runner {
    fn run(&self) -> Result<(), Error> {
        let project_path = PathBuf::from(&self.args.project_path);
        self.print
            .infoln(format!("Initializing project at {project_path:?}"));

        // create a project dir, and copy the contents of the base template (contract-init-template) into it
        Self::create_dir_all(&project_path)?;
        self.copy_template_files()?;

        Ok(())
    }

    fn copy_template_files(&self) -> Result<(), Error> {
        let project_path = Path::new(&self.args.project_path);
        for item in TemplateFiles::iter() {
            let mut to = project_path.join(item.as_ref());
            let exists = Self::file_exists(&to);
            if exists && !self.args.overwrite {
                self.print
                    .infoln(format!("Skipped creating {to:?} as it already exists"));
                continue;
            }

            Self::create_dir_all(to.parent().unwrap())?;

            let Some(file) = TemplateFiles::get(item.as_ref()) else {
                self.print
                    .warnln(format!("Failed to read file: {}", item.as_ref()));
                continue;
            };

            let file_contents =
                str::from_utf8(file.data.as_ref()).map_err(Error::ConvertBytesToString)?;

            // We need to include the Cargo.toml file as Cargo.toml.removeextension in the template so that it will be included the package. This is making sure that the Cargo file is written as Cargo.toml in the new project. This is a workaround for this issue: https://github.com/rust-lang/cargo/issues/8597.
            let item_path = Path::new(item.as_ref());
            if item_path.file_name().unwrap() == "Cargo.toml.removeextension" {
                let item_parent_path = item_path.parent().unwrap();
                to = project_path.join(item_parent_path).join("Cargo.toml");
            }

            if exists {
                self.print
                    .plusln(format!("Writing {to:?} (overwriting existing file)"));
            } else {
                self.print.plusln(format!("Writing {to:?}"));
            }
            Self::write(&to, file_contents)?;
        }

        Self::create_dir_all(project_path.join("contracts").as_path())?;

        Ok(())
    }

    fn file_exists(file_path: &Path) -> bool {
        metadata(file_path)
            .as_ref()
            .map(Metadata::is_file)
            .unwrap_or(false)
    }

    fn create_dir_all(path: &Path) -> Result<(), Error> {
        create_dir_all(path).map_err(|e| Error::Io(format!("creating directory: {path:?}"), e))
    }

    fn write(path: &Path, contents: &str) -> Result<(), Error> {
        write(path, contents).map_err(|e| Error::Io(format!("writing file: {path:?}"), e))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::read_to_string;

    use itertools::Itertools;

    use super::*;

    const TEST_PROJECT_NAME: &str = "test-project";

    #[test]
    fn test_init() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_dir = temp_dir.path().join(TEST_PROJECT_NAME);
        let runner = Runner {
            args: Cmd {
                project_path: project_dir.to_string_lossy().to_string(),
                with_example: None,
                frontend_template: None,
                overwrite: false,
            },
            print: print::Print::new(false),
        };
        runner.run().unwrap();

        assert_base_template_files_exist(&project_dir);
        assert_default_hello_world_contract_files_exist(&project_dir);
        assert_excluded_paths_do_not_exist(&project_dir);

        assert_contract_cargo_file_is_well_formed(&project_dir, "hello_world");

        assert_excluded_paths_do_not_exist(&project_dir);

        temp_dir.close().unwrap();
    }

    // test helpers
    fn assert_base_template_files_exist(project_dir: &Path) {
        let expected_paths = ["contracts", "Cargo.toml", "README.md"];
        for path in &expected_paths {
            assert!(project_dir.join(path).exists());
        }
    }

    fn assert_default_hello_world_contract_files_exist(project_dir: &Path) {
        assert_contract_files_exist(project_dir, "hello_world");
    }

    fn assert_contract_files_exist(project_dir: &Path, contract_name: &str) {
        let contract_dir = project_dir.join("contracts").join(contract_name);

        assert!(contract_dir.exists());
        assert!(contract_dir.as_path().join("Cargo.toml").exists());
        assert!(contract_dir.as_path().join("src").join("lib.rs").exists());
        assert!(contract_dir.as_path().join("src").join("test.rs").exists());
    }

    fn assert_contract_cargo_file_is_well_formed(project_dir: &Path, contract_name: &str) {
        let contract_dir = project_dir.join("contracts").join(contract_name);
        let cargo_toml_path = contract_dir.as_path().join("Cargo.toml");
        let cargo_toml_str = read_to_string(cargo_toml_path.clone()).unwrap();
        let doc = cargo_toml_str.parse::<toml_edit::Document>().unwrap();
        assert!(
            doc.get("dependencies")
                .unwrap()
                .get("soroban-sdk")
                .unwrap()
                .get("workspace")
                .unwrap()
                .as_bool()
                .unwrap(),
            "expected [dependencies.soroban-sdk] to be a workspace dependency"
        );
        assert!(
            doc.get("dev-dependencies")
                .unwrap()
                .get("soroban-sdk")
                .unwrap()
                .get("workspace")
                .unwrap()
                .as_bool()
                .unwrap(),
            "expected [dev-dependencies.soroban-sdk] to be a workspace dependency"
        );
        assert_ne!(
            0,
            doc.get("dev-dependencies")
                .unwrap()
                .get("soroban-sdk")
                .unwrap()
                .get("features")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            "expected [dev-dependencies.soroban-sdk] to have a features list"
        );
        assert!(
            doc.get("dev_dependencies").is_none(),
            "erroneous 'dev_dependencies' section"
        );
        assert_eq!(
            doc.get("lib")
                .unwrap()
                .get("crate-type")
                .unwrap()
                .as_array()
                .unwrap()
                .get(0)
                .unwrap()
                .as_str()
                .unwrap(),
            "cdylib",
            "expected [lib.crate-type] to be 'cdylib'"
        );
    }

    fn assert_excluded_paths_do_not_exist(project_dir: &Path) {
        let base_excluded_paths = [".git", ".github", "Makefile", ".vscode", "target"];
        for path in &base_excluded_paths {
            let filepath = project_dir.join(path);
            assert!(!filepath.exists(), "{filepath:?} should not exist");
        }
        let contract_excluded_paths = ["Makefile", "target", "Cargo.lock"];
        let contract_dirs = fs::read_dir(project_dir.join("contracts"))
            .unwrap()
            .map(|entry| entry.unwrap().path());
        contract_dirs
            .cartesian_product(contract_excluded_paths.iter())
            .for_each(|(contract_dir, excluded_path)| {
                let filepath = contract_dir.join(excluded_path);
                assert!(!filepath.exists(), "{filepath:?} should not exist");
            });
    }
}
