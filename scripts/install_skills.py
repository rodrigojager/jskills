"""Copy the jskills collection into one supported agent's skills directory."""

import argparse
import os
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
HOME = Path.home()
SKILLS = (
    "audit-other-agent",
    "decision-brief",
    "existing-solutions-scout",
    "fix-tool",
    "grill-with-docs",
    "plan-to-html",
    "ship-release",
    "to-plan",
)
TASK_SKILLS = ("estimate-jtasks", "jtasks", "to-jtasks")
OPTIONAL_SKILLS = {"jdeslop": "jdeslop", "jjuicer": "juice-it-up"}
IGNORED = shutil.ignore_patterns(
    ".git", ".vendor", "__pycache__", "*.pyc", ".test-output", "node_modules",
    ".juice-it-up", "target",
)


def agent_directory(agent: str) -> Path:
    defaults = {
        "claude": Path(os.environ.get("CLAUDE_CONFIG_DIR") or HOME / ".claude"),
        "codex": Path(os.environ.get("CODEX_HOME") or HOME / ".codex"),
        "opencode": Path(os.environ.get("XDG_CONFIG_HOME") or HOME / ".config") / "opencode",
    }
    return defaults[agent] / "skills"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("agent", choices=("claude", "codex", "opencode"))
    parser.add_argument(
        "--optional", nargs="+", action="extend", choices=OPTIONAL_SKILLS,
        help="Install only the named optional skills. Omit this option for the standard collection.",
    )
    parser.add_argument("--destination", type=Path, help="Override the agent's skills directory.")
    parser.add_argument("--dry-run", action="store_true", help="List the destinations without writing.")
    args = parser.parse_args()
    destination = (args.destination or agent_directory(args.agent)).expanduser().resolve()
    if args.optional:
        sources = [ROOT / "skills" / OPTIONAL_SKILLS[name] for name in dict.fromkeys(args.optional)]
    else:
        sources = [ROOT / "skills" / name for name in SKILLS]
        sources.append(ROOT / "tools" / "jproject" / "skills" / "jproject")
        sources.extend(ROOT / "tools" / "jtasks" / "skills" / args.agent / name for name in TASK_SKILLS)

    if destination.exists() and not destination.is_dir():
        parser.error(f"The destination is not a directory: {destination}")
    for source in sources:
        if not (source / "SKILL.md").is_file():
            parser.error(f"The source skill is missing: {source}")
    conflicts = [destination / source.name for source in sources if os.path.lexists(destination / source.name)]
    if conflicts and not args.dry_run:
        parser.error("Choose another skills directory. These entries already exist:\n" + "\n".join(map(str, conflicts)))

    for source in sorted(sources):
        target = destination / source.name
        if args.dry_run:
            state = "already exists" if target in conflicts else "ready"
            print(f"{target} ({state})")
        else:
            shutil.copytree(source, target, ignore=IGNORED)
            print(f"Installed {target}")


if __name__ == "__main__":
    main()
