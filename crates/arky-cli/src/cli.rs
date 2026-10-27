//! The command line: the commands, their options and the values they take.

use core::fmt;
use core::str::FromStr;
use std::path::PathBuf;

use arky::forms::Form;
use arky::parser::ast::FrontmatterFormat;

#[derive(Debug, clap::Subcommand)]
pub enum AssetCommand {
    /// Finishes each deletion of an embedded file that stopped halfway. Run it only while no other
    /// arky writes to the store.
    FinishDeletions {
        /// The MongoDB store, `mongodb:<database>/<collection>`.
        #[arg(short, long)]
        store: StoreArg,
    },
}

#[derive(Debug, clap::Parser)]
#[command(name = "arky", version, about = "Markdown documents as a typed tree")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// The configuration file, in place of `ARKY_CONFIG` and of `arky.toml` in the current
    /// folder.
    #[arg(short = 'c', long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Commands on the files that documents embed.
    Asset {
        #[command(subcommand)]
        command: AssetCommand,
    },
    /// Repairs what needs no decision: removes each lock a save left behind that has expired.
    Fix {
        /// The folder to repair, the current folder when it is left out.
        path: Option<PathBuf>,
        /// The store PATH is in, in place of the file system.
        #[arg(short, long)]
        store: Option<StoreArg>,
    },
    /// Writes the document at PATH to standard output, or to --output.
    Get {
        path: PathBuf,
        /// Gives the document though a file it embeds is missing or leaves its folder, its embed
        /// kept as written.
        #[arg(long)]
        allow_missing: bool,
        /// Writes markdown to --output as one file, every embedded file inside it as a data URI.
        #[arg(long, requires = "output")]
        embed: bool,
        /// The form the document is written in.
        #[arg(short, long, value_enum, default_value_t = FormatArg::Markdown)]
        format: FormatArg,
        /// The syntax of the frontmatter, in the markdown and zip forms.
        #[arg(long, value_enum, value_name = "SYNTAX", default_value_t = SyntaxArg::Yaml)]
        frontmatter: SyntaxArg,
        /// The file the document is written to, in place of standard output.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// The store PATH is in, in place of the file system.
        #[arg(short, long)]
        store: Option<StoreArg>,
    },
    /// Stores at PATH the document read from standard input, or from --input.
    Write {
        path: PathBuf,
        /// Stores the document though a file it embeds is missing or leaves its folder, its embed
        /// kept as written.
        #[arg(long)]
        allow_missing: bool,
        /// The form the document is read in.
        #[arg(short, long, value_enum, default_value_t = FormatArg::Markdown)]
        format: FormatArg,
        /// The syntax of the frontmatter stored at PATH.
        #[arg(long, value_enum, value_name = "SYNTAX", default_value_t = SyntaxArg::Yaml)]
        frontmatter: SyntaxArg,
        /// The file the document is read from, in place of standard input.
        #[arg(short, long, value_name = "FILE")]
        input: Option<PathBuf>,
        /// The store PATH is in, in place of the file system.
        #[arg(short, long)]
        store: Option<StoreArg>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FormatArg {
    Binary,
    Json,
    Markdown,
    Zip,
}

impl From<FormatArg> for Form {
    fn from(format: FormatArg) -> Self {
        match format {
            FormatArg::Binary => Self::Binary,
            FormatArg::Json => Self::Json,
            FormatArg::Markdown => Self::Markdown,
            FormatArg::Zip => Self::Zip,
        }
    }
}

/// A store named on the command line, a place inside a server:
/// `minio:<bucket>`, `minio:<bucket>/<prefix>` or `mongodb:<database>/<collection>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreArg {
    Minio {
        bucket: String,
        prefix: Option<String>,
    },
    Mongodb {
        collection: String,
        database: String,
    },
}

impl StoreArg {
    /// The store of `minio:` and `place`, `<bucket>` or `<bucket>/<prefix>`.
    fn minio(place: &str) -> Option<Self> {
        bucket_and_prefix(place).map(|(bucket, prefix)| Self::Minio {
            bucket: bucket.to_owned(),
            prefix: prefix.map(str::to_owned),
        })
    }

    /// The store of `mongodb:` and `place`, `<database>/<collection>`.
    fn mongodb(place: &str) -> Option<Self> {
        place
            .split_once('/')
            .filter(|(database, collection)| !database.is_empty() && !collection.is_empty())
            .map(|(database, collection)| Self::Mongodb {
                collection: collection.to_owned(),
                database: database.to_owned(),
            })
    }
}

impl fmt::Display for StoreArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Minio {
                bucket,
                prefix: None,
            } => write!(f, "minio:{bucket}"),
            Self::Minio {
                bucket,
                prefix: Some(prefix),
            } => write!(f, "minio:{bucket}/{prefix}"),
            Self::Mongodb {
                collection,
                database,
            } => write!(f, "mongodb:{database}/{collection}"),
        }
    }
}

impl FromStr for StoreArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let store = match text.split_once(':') {
            Some(("minio", place)) => Self::minio(place),
            Some(("mongodb", place)) => Self::mongodb(place),
            Some(_) | None => None,
        };
        store.ok_or_else(|| {
            "a store is minio:<bucket>, minio:<bucket>/<prefix> or mongodb:<database>/<collection>"
                .to_owned()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SyntaxArg {
    Toml,
    Yaml,
}

impl From<SyntaxArg> for FrontmatterFormat {
    fn from(syntax: SyntaxArg) -> Self {
        match syntax {
            SyntaxArg::Toml => Self::Toml,
            SyntaxArg::Yaml => Self::Yaml,
        }
    }
}

/// The bucket and the key prefix of `place`, `<bucket>` or `<bucket>/<prefix>`,
/// neither empty.
pub fn bucket_and_prefix(place: &str) -> Option<(&str, Option<&str>)> {
    let (bucket, prefix) = place
        .split_once('/')
        .map_or((place, None), |(bucket, prefix)| (bucket, Some(prefix)));
    (!bucket.is_empty() && prefix.is_none_or(|part| !part.is_empty())).then_some((bucket, prefix))
}

#[cfg(test)]
mod tests;
