//! The command line: the commands, their options and the values they take.

use std::path::PathBuf;

use arky::forms::Form;
use arky::parser::ast::FrontmatterFormat;

#[derive(Debug, clap::Subcommand)]
pub enum AssetCommand {
    /// Finishes each deletion of an embedded file that stopped halfway. Run it only while no other
    /// arky writes to the store.
    FinishDeletions {
        /// The store: a store string or arky://<name>. Only a MongoDB store keeps deletions.
        #[arg(short, long)]
        store: StoreArg,
    },
}

#[derive(Debug, clap::Parser)]
#[command(name = "arky", version, about = "Markdown documents as a typed tree")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// The configuration file, read alone, in place of `ARKY_CONFIG` and of the `arky.toml`
    /// files from the current folder up.
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
        /// The folder to repair: in the file system, the current folder when it is left out; in
        /// --store, the whole store when it is left out.
        path: Option<PathBuf>,
        /// The store PATH is in: a store string, arky://<name> or a folder, in place of the file
        /// system. A path written after the store is a folder inside it.
        #[arg(short, long)]
        store: Option<StoreArg>,
    },
    /// Writes the document at PATH to standard output, or to --output. PATH may be a store
    /// string, such as arky://<name>/<path>.
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
        /// The store PATH is in: a store string, arky://<name> or a folder, in place of
        /// `ARKY_STORE`, the default of `arky.toml` and the file system. A path written after the
        /// store is a folder inside it; --store never names the document.
        #[arg(short, long)]
        store: Option<StoreArg>,
    },
    /// Commands on the saved stores of arky.toml.
    Store {
        #[command(subcommand)]
        command: StoreCommand,
    },
    /// Stores at PATH the document read from standard input, or from --input. PATH may be a
    /// store string, such as arky://<name>/<path>.
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
        /// The store PATH is in: a store string, arky://<name> or a folder, in place of
        /// `ARKY_STORE`, the default of `arky.toml` and the file system. A path written after the
        /// store is a folder inside it; --store never names the document.
        #[arg(short, long)]
        store: Option<StoreArg>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FormatArg {
    #[cfg(feature = "binary-format")]
    Binary,
    Json,
    Markdown,
    Zip,
}

impl From<FormatArg> for Form {
    fn from(format: FormatArg) -> Self {
        match format {
            #[cfg(feature = "binary-format")]
            FormatArg::Binary => Self::Binary,
            FormatArg::Json => Self::Json,
            FormatArg::Markdown => Self::Markdown,
            FormatArg::Zip => Self::Zip,
        }
    }
}

/// A store as the command line names it: a store string
/// (`minio+https://minio.example.com/acme/vault`), a saved store (`arky://notes`) or a
/// folder (`./notes`). The backends read it, so any text parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreArg(pub String);

impl From<String> for StoreArg {
    fn from(text: String) -> Self {
        Self(text)
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum StoreCommand {
    /// Lists the saved stores in use here: each name, its URL as written and the arky.toml it
    /// comes from.
    List,
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

#[cfg(test)]
mod tests;
