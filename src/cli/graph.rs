use crate::{
    domain::graph::Proposal,
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request},
};
use anyhow::{Context, Result, ensure};
use clap::{Args, Subcommand};
use std::{fs, path::PathBuf};

#[derive(Args)]
pub struct Graph {
    #[command(subcommand)]
    pub action: Action,
}
#[derive(Subcommand)]
pub enum Action {
    /// Apply precise pending intent under a validated base revision.
    Propose {
        #[arg(long)]
        run: String,
        #[arg(long)]
        file: PathBuf,
    },
}
pub fn execute(workspace: &Workspace, args: Graph) -> Result<i32> {
    let Action::Propose { run, file } = args.action;
    ensure!(
        workspace.database().is_file(),
        "no runtime database; import first"
    );
    let proposal: Proposal = serde_json::from_slice(&fs::read(file)?)?;
    let receipt = match ControllerGuard::acquire(&workspace.git_dir) {
        Ok(guard) => {
            let store = Store::open(workspace, &guard)?;
            crate::controller::graph::propose(workspace, &store, &run, &proposal)?
        }
        Err(_) => {
            let endpoint: ControlEndpoint = serde_json::from_slice(
                &fs::read(workspace.data.join("control.json"))
                    .context("checkout writer busy; task controller unavailable")?,
            )?;
            ensure!(endpoint.run == run, "controller serves another run");
            send(workspace, endpoint, run, proposal)?
                .graph
                .context("controller returned no graph receipt")?
        }
    };
    println!("{receipt}");
    Ok(0)
}

/// Send inline when the request fits one control message; otherwise pass the
/// proposal by reference through the data folder and remove the file after.
fn send(
    workspace: &Workspace,
    endpoint: ControlEndpoint,
    run: String,
    proposal: Proposal,
) -> Result<transport::Response> {
    let inline = Request::Graph {
        token: endpoint.token,
        run,
        proposal: Box::new(proposal),
    };
    if transport::fits(&inline)? {
        return transport::send(endpoint.endpoint, &inline);
    }
    let Request::Graph {
        token,
        run,
        proposal,
    } = inline
    else {
        unreachable!()
    };
    let (path, sha256) = crate::controller::graph::write_referenced(workspace, &proposal)?;
    let response = transport::send(
        endpoint.endpoint,
        &Request::GraphFile {
            token,
            run,
            path: path.clone(),
            sha256,
        },
    );
    let _ = fs::remove_file(&path);
    response
}
