use clap::{Parser, Subcommand};
use editorconfig_lint::{check, Config};
use std::{io::BufReader, path::PathBuf};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Check {
        #[arg(index(1))]
        file_path: PathBuf,
    },
    Fix {},
    ShowConfig {
        #[arg(index(1))]
        file_path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::try_parse()?;

    match cli.command {
        Command::ShowConfig { file_path } => {
            println!("{:#?}", Config::get_config_for(&file_path)?);
        }
        Command::Check { file_path } => {
            let config = Config::get_config_for(&file_path)?;
            let reader = BufReader::new(std::fs::File::open(&file_path)?);
            let diagnoses = check(reader, config)?;
            let mut stdout = std::io::stdout().lock();
            for diagnosis in diagnoses {
                diagnosis.fmt(&mut stdout, &file_path.display())?;
            }
        }
        Command::Fix {} => todo!(),
    }

    Ok(())
}
