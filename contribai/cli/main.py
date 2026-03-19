"""ContribAI CLI - Rich command-line interface.

Usage:
    contribai run           Auto-discover repos and contribute
    contribai target <url>  Target a specific repo
    contribai analyze <url> Analyze a repo without contributing
    contribai solve <url>   Solve issues in a specific repo
    contribai status        Show status of submitted PRs
    contribai stats         Show overall statistics
    contribai config        Show current configuration
"""

from __future__ import annotations

import asyncio
import logging
import sys

import click
from rich.console import Console
from rich.logging import RichHandler
from rich.panel import Panel
from rich.table import Table

from contribai import __version__
from contribai.core.config import load_config

# Fix Windows console encoding for emoji/unicode support
if sys.platform == "win32":
    import os

    os.environ.setdefault("PYTHONIOENCODING", "utf-8")
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")

console = Console()


def setup_logging(verbose: bool = False):
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(message)s",
        handlers=[RichHandler(console=console, show_path=False, rich_tracebacks=True)],
    )


def print_banner():
    banner = f"""[bold cyan]
   ____            _        _ _      _    ___
  / ___|___  _ __ | |_ _ __(_) |__  / \\  |_ _|
 | |   / _ \\| '_ \\| __| '__| | '_ \\/ _ \\  | |
 | |__| (_) | | | | |_| |  | | |_) / ___ \\ | |
  \\____\\___/|_| |_|\\__|_|  |_|_.__/_/   \\_\\___|

  [dim]AI Agent for Open Source Contributions v{__version__}[/dim]
[/bold cyan]"""
    console.print(banner)


@click.group()
@click.option("--config", "-c", type=click.Path(), default=None, help="Config file path")
@click.option("--verbose", "-v", is_flag=True, help="Enable verbose logging")
@click.pass_context
def cli(ctx, config, verbose):
    """ContribAI - AI Agent that contributes to open source projects."""
    ctx.ensure_object(dict)
    setup_logging(verbose)
    ctx.obj["config_path"] = config
    ctx.obj["verbose"] = verbose


@cli.command()
@click.option("--language", "-l", multiple=True, help="Filter by language(s)")
@click.option("--stars", "-s", default=None, help="Star range (e.g., 100-5000)")
@click.option("--max-prs", "-m", type=int, default=None, help="Max PRs to create")
@click.option("--dry-run", is_flag=True, help="Analyze and generate without creating PRs")
@click.pass_context
def run(ctx, language, stars, max_prs, dry_run):
    """Auto-discover repositories and create contributions."""
    print_banner()

    config = load_config(ctx.obj["config_path"])

    # Apply CLI overrides
    if language:
        config.discovery.languages = list(language)
    if stars:
        parts = stars.split("-")
        config.discovery.stars_range = [int(parts[0]), int(parts[1])]
    if max_prs:
        config.github.max_prs_per_day = max_prs

    if not config.github.token:
        console.print("[red]❌ GitHub token not configured![/red]")
        console.print("Set it in config.yaml or run: contribai config set github.token <token>")
        sys.exit(1)

    if not config.llm.api_key and not config.llm.use_vertex:
        console.print("[red]❌ LLM API key not configured![/red]")
        sys.exit(1)

    mode = "[yellow]DRY RUN[/yellow]" if dry_run else "[green]LIVE[/green]"
    console.print(f"\n🚀 Starting ContribAI pipeline ({mode})")
    console.print(f"   Languages: {', '.join(config.discovery.languages)}")
    console.print(f"   Stars: {config.discovery.stars_range[0]}-{config.discovery.stars_range[1]}")
    console.print(f"   LLM: {config.llm.provider} ({config.llm.model})")
    console.print()

    from contribai.orchestrator.pipeline import ContribPipeline

    pipeline = ContribPipeline(config)
    result = asyncio.run(pipeline.run(dry_run=dry_run))

    # Print results
    _print_result(result, dry_run)


@cli.command()
@click.argument("url")
@click.option("--types", "-t", default=None, help="Contribution types (comma-separated)")
@click.option("--dry-run", is_flag=True, help="Analyze and generate without creating PRs")
@click.pass_context
def target(ctx, url, types, dry_run):
    """Target a specific repository for contributions."""
    print_banner()

    config = load_config(ctx.obj["config_path"])

    if types:
        config.contribution.enabled_types = types.split(",")

    if not config.github.token:
        console.print("[red]❌ GitHub token not configured![/red]")
        sys.exit(1)

    if not config.llm.api_key and not config.llm.use_vertex:
        console.print("[red]❌ LLM API key not configured![/red]")
        sys.exit(1)

    mode = "[yellow]DRY RUN[/yellow]" if dry_run else "[green]LIVE[/green]"
    console.print(f"\n🎯 Targeting: {url} ({mode})")
    console.print(f"   LLM: {config.llm.provider} ({config.llm.model})\n")

    from contribai.orchestrator.pipeline import ContribPipeline

    pipeline = ContribPipeline(config)
    result = asyncio.run(pipeline.run_single(url, dry_run=dry_run))
    _print_result(result, dry_run)


@cli.command()
@click.argument("url")
@click.pass_context
def analyze(ctx, url):
    """Analyze a repository without creating contributions."""
    print_banner()

    config = load_config(ctx.obj["config_path"])

    if not config.github.token:
        console.print("[red]❌ GitHub token not configured![/red]")
        sys.exit(1)

    if not config.llm.api_key and not config.llm.use_vertex:
        console.print("[red]❌ LLM API key not configured![/red]")
        sys.exit(1)

    console.print(f"\n🔬 Analyzing: {url}")
    console.print(f"   LLM: {config.llm.provider} ({config.llm.model})\n")

    from contribai.orchestrator.pipeline import ContribPipeline

    pipeline = ContribPipeline(config)
    result = asyncio.run(pipeline.analyze_only(url))

    if not result:
        console.print("[red]Analysis failed[/red]")
        return

    # Display findings
    console.print(
        Panel(
            f"Analyzed [bold]{result.analyzed_files}[/bold] files "
            f"in [bold]{result.analysis_duration_sec:.1f}s[/bold]\n"
            f"Skipped {result.skipped_files} files\n"
            f"Found [bold]{len(result.findings)}[/bold] issues",
            title=f"📊 Analysis: {result.repo.full_name}",
        )
    )

    if result.findings:
        table = Table(title="Findings", show_lines=True)
        table.add_column("Severity", style="bold", width=10)
        table.add_column("Type", width=15)
        table.add_column("Title", width=35)
        table.add_column("File", width=25)

        severity_colors = {
            "critical": "red",
            "high": "yellow",
            "medium": "cyan",
            "low": "dim",
        }

        for f in result.top_findings:
            color = severity_colors.get(f.severity.value, "white")
            table.add_row(
                f"[{color}]{f.severity.value.upper()}[/{color}]",
                f.type.value,
                f.title,
                f.file_path[:25],
            )

        console.print(table)


@cli.command()
@click.argument("url")
@click.option("--max-issues", "-n", type=int, default=5, help="Max issues to process")
@click.option("--dry-run", is_flag=True, help="Analyze issues without creating PRs")
@click.pass_context
def solve(ctx, url, max_issues, dry_run):
    """Solve open issues in a specific repository."""
    print_banner()

    config = load_config(ctx.obj["config_path"])

    if not config.github.token:
        console.print("[red]❌ GitHub token not configured![/red]")
        sys.exit(1)

    if not config.llm.api_key and not config.llm.use_vertex:
        console.print("[red]❌ LLM API key not configured![/red]")
        sys.exit(1)

    mode = "[yellow]DRY RUN[/yellow]" if dry_run else "[green]LIVE[/green]"
    console.print(f"\n🎯 Solving issues in: {url} ({mode})")
    console.print(f"   Max issues: {max_issues}")
    console.print(f"   LLM: {config.llm.provider} ({config.llm.model})\n")

    from contribai.github.client import GitHubClient
    from contribai.issues.solver import IssueSolver
    from contribai.llm.provider import create_llm_provider

    async def _solve():
        parts = url.rstrip("/").split("/")
        owner, repo_name = parts[-2], parts[-1]

        llm = create_llm_provider(config.llm)
        github = GitHubClient(token=config.github.token)

        try:
            _repo = await github.get_repo_details(owner, repo_name)
            issues = await github.get_open_issues(owner, repo_name, per_page=20)

            solver = IssueSolver(llm=llm, github=github)
            solvable = solver.filter_solvable(issues, max_complexity=3)[:max_issues]

            console.print(f"Found [bold]{len(issues)}[/bold] open issues")
            console.print(f"[green]{len(solvable)}[/green] are solvable\n")

            if not solvable:
                console.print("[dim]No solvable issues found.[/dim]")
                return

            table = Table(title="🎯 Solvable Issues", show_lines=True)
            table.add_column("#", width=5)
            table.add_column("Category", width=12)
            table.add_column("Title", width=40)
            table.add_column("Labels", width=15)

            for issue in solvable:
                cat = solver.classify_issue(issue)
                table.add_row(
                    str(issue.number),
                    cat.value,
                    issue.title[:40],
                    ", ".join(issue.labels[:2]) or "-",
                )

            console.print(table)

        finally:
            await github.close()
            await llm.close()

    asyncio.run(_solve())


@cli.command()
@click.option("--status-filter", "-s", default=None, help="Filter by PR status")
@click.pass_context
def status(ctx, status_filter):
    """Show status of submitted pull requests."""
    print_banner()

    config = load_config(ctx.obj["config_path"])

    from contribai.orchestrator.memory import Memory

    async def _show():
        memory = Memory(config.storage.resolved_db_path)
        await memory.init()
        prs = await memory.get_prs(status=status_filter)
        await memory.close()
        return prs

    prs = asyncio.run(_show())

    if not prs:
        console.print("[dim]No PRs found.[/dim]")
        return

    table = Table(title="📋 Submitted PRs", show_lines=True)
    table.add_column("#", width=5)
    table.add_column("Repository", width=25)
    table.add_column("Title", width=35)
    table.add_column("Status", width=10)
    table.add_column("URL", width=30)

    status_colors = {
        "open": "green",
        "merged": "magenta",
        "closed": "red",
        "pending": "yellow",
    }

    for pr in prs:
        color = status_colors.get(pr["status"], "white")
        table.add_row(
            str(pr["pr_number"]),
            pr["repo"],
            pr["title"][:35],
            f"[{color}]{pr['status'].upper()}[/{color}]",
            pr["pr_url"],
        )

    console.print(table)


@cli.command()
@click.pass_context
def stats(ctx):
    """Show overall ContribAI statistics."""
    print_banner()

    config = load_config(ctx.obj["config_path"])

    from contribai.orchestrator.memory import Memory

    async def _get_stats():
        memory = Memory(config.storage.resolved_db_path)
        await memory.init()
        s = await memory.get_stats()
        await memory.close()
        return s

    s = asyncio.run(_get_stats())

    console.print(
        Panel(
            f"📊 [bold]Total Runs[/bold]: {s['total_runs']}\n"
            f"🔬 [bold]Repos Analyzed[/bold]: {s['total_repos_analyzed']}\n"
            f"📤 [bold]PRs Submitted[/bold]: {s['total_prs_submitted']}\n"
            f"✅ [bold]PRs Merged[/bold]: {s['prs_merged']}",
            title="ContribAI Statistics",
        )
    )


@cli.command("config")
@click.pass_context
def show_config(ctx):
    """Show current configuration."""
    config = load_config(ctx.obj["config_path"])
    console.print(
        Panel(
            f"[bold]GitHub[/bold]\n"
            f"  Token: {'****' + config.github.token[-4:] if config.github.token else 'NOT SET'}\n"
            f"  Max repos/run: {config.github.max_repos_per_run}\n"
            f"  Max PRs/day: {config.github.max_prs_per_day}\n\n"
            f"[bold]LLM[/bold]\n"
            f"  Provider: {config.llm.provider}\n"
            f"  Model: {config.llm.model}\n"
            f"  API Key: "
            f"{'****' + config.llm.api_key[-4:] if config.llm.api_key else 'NOT SET'}\n\n"
            f"[bold]Discovery[/bold]\n"
            f"  Languages: {', '.join(config.discovery.languages)}\n"
            f"  Stars: {config.discovery.stars_range}\n\n"
            f"[bold]Analysis[/bold]\n"
            f"  Analyzers: {', '.join(config.analysis.enabled_analyzers)}\n"
            f"  Threshold: {config.analysis.severity_threshold}\n\n"
            f"[bold]Pipeline[/bold]\n"
            f"  Max concurrent: {config.pipeline.max_concurrent_repos}\n"
            f"  Timeout/repo: {config.pipeline.timeout_per_repo_sec}s\n\n"
            f"[bold]Web Dashboard[/bold]\n"
            f"  Host: {config.web.host}:{config.web.port}\n\n"
            f"[bold]Scheduler[/bold]\n"
            f"  Enabled: {config.scheduler.enabled}\n"
            f"  Cron: {config.scheduler.cron}",
            title="ContribAI Configuration",
        )
    )


@cli.command("serve")
@click.option("--host", default=None, help="Host to bind to")
@click.option("--port", default=None, type=int, help="Port")
@click.pass_context
def serve(ctx, host, port):
    """Start the web dashboard server."""
    config = load_config(ctx.obj["config_path"])
    if host:
        config.web.host = host
    if port:
        config.web.port = port

    console.print(
        f"[bold]Starting ContribAI Dashboard[/bold] at http://{config.web.host}:{config.web.port}"
    )

    from contribai.web.server import run_server

    run_server(config)


@cli.command("schedule")
@click.option("--cron", default=None, help="Cron expression")
@click.pass_context
def schedule(ctx, cron):
    """Start the scheduler daemon for automated runs."""
    config = load_config(ctx.obj["config_path"])
    config.scheduler.enabled = True
    if cron:
        config.scheduler.cron = cron

    console.print(
        f"[bold]Starting ContribAI Scheduler[/bold]\n"
        f"  Cron: {config.scheduler.cron}\n"
        f"  Timezone: {config.scheduler.timezone}"
    )

    from contribai.scheduler.scheduler import (
        ContribScheduler,
    )

    sched = ContribScheduler(config)
    sched.start()


@cli.command("templates")
@click.option(
    "--type",
    "contrib_type",
    default=None,
    help="Filter by contribution type",
)
def list_templates(contrib_type):
    """List available contribution templates."""
    from contribai.templates.registry import (
        TemplateRegistry,
    )

    registry = TemplateRegistry()
    templates = registry.filter_by_type(contrib_type) if contrib_type else registry.list_all()

    if not templates:
        console.print("[yellow]No templates found[/yellow]")
        return

    table = Table(title="Contribution Templates")
    table.add_column("Name", style="cyan")
    table.add_column("Type", style="green")
    table.add_column("Severity")
    table.add_column("Description")
    table.add_column("Languages")

    for t in templates:
        table.add_row(
            t.name,
            t.type,
            t.severity,
            t.description,
            ", ".join(t.languages) if t.languages else "all",
        )

    console.print(table)


@cli.command("profile")
@click.argument("name")
@click.option(
    "--dry-run",
    is_flag=True,
    help="Analyze only, no PRs",
)
@click.option(
    "--list",
    "list_all",
    is_flag=True,
    help="List all profiles",
)
@click.pass_context
def run_profile(ctx, name, dry_run, list_all):
    """Run pipeline with a named profile."""
    from contribai.core.profiles import (
        get_profile,
        list_profiles,
    )

    if list_all or name == "list":
        profiles = list_profiles()
        table = Table(title="Contribution Profiles")
        table.add_column("Name", style="cyan")
        table.add_column("Description")
        table.add_column("Analyzers")
        table.add_column("Threshold")
        for p in profiles:
            table.add_row(
                p.name,
                p.description,
                ", ".join(p.analyzers),
                p.severity_threshold,
            )
        console.print(table)
        return

    profile = get_profile(name)
    if not profile:
        console.print(f"[red]Profile '{name}' not found[/red]")
        console.print("Available: " + ", ".join(p.name for p in list_profiles()))
        return

    console.print(f"[bold]Running with profile: {profile.name}[/bold]\n  {profile.description}")

    from contribai.core.profiles import apply_profile

    config = load_config(ctx.obj["config_path"])
    config_data = config.model_dump()
    config_data = apply_profile(config_data, profile)

    from contribai.core.config import ContribAIConfig

    config = ContribAIConfig(**config_data)

    if profile.dry_run or dry_run:
        dry_run = True

    from contribai.orchestrator.pipeline import (
        ContribPipeline,
    )

    pipeline = ContribPipeline(config)
    result = asyncio.run(pipeline.run(dry_run=dry_run))
    _print_result(result, dry_run)


def _print_result(result, dry_run: bool):
    """Print pipeline execution results."""

    console.print()
    console.print(
        Panel(
            f"📦 Repos analyzed: [bold]{result.repos_analyzed}[/bold]\n"
            f"🔍 Issues found: [bold]{result.findings_total}[/bold]\n"
            f"🛠️  Contributions generated: [bold]{result.contributions_generated}[/bold]\n"
            f"📤 PRs created: [bold]{result.prs_created}[/bold]"
            + (f"\n❌ Errors: [red]{len(result.errors)}[/red]" if result.errors else ""),
            title="✅ Pipeline Complete" + (" (DRY RUN)" if dry_run else ""),
        )
    )

    if result.prs:
        console.print("\n[bold]Created PRs:[/bold]")
        for pr in result.prs:
            console.print(f"  • [green]{pr.pr_url}[/green] - {pr.contribution.title}")

    if result.errors:
        console.print("\n[bold red]Errors:[/bold red]")
        for e in result.errors:
            console.print(f"  • {e}")


@cli.command("models")
@click.option("--task", default=None, help="Filter by task type")
@click.pass_context
def show_models(ctx, task):
    """List available models and their capabilities."""
    from contribai.llm.models import (
        ALL_MODELS,
        TaskType,
        get_models_for_task,
    )
    from contribai.llm.router import TaskRouter

    if task:
        try:
            tt = TaskType(task)
        except ValueError:
            console.print(
                f"[red]Unknown task type: {task}[/red]\n"
                f"Valid: {', '.join(t.value for t in TaskType)}"
            )
            return
        models = get_models_for_task(tt)
        console.print(f"\n[bold]Best models for [cyan]{task}[/cyan]:[/bold]\n")
    else:
        models = ALL_MODELS
        console.print("\n[bold]Available Models:[/bold]\n")

    table = Table()
    table.add_column("Model", style="cyan")
    table.add_column("Tier")
    table.add_column("Code", justify="right")
    table.add_column("Analysis", justify="right")
    table.add_column("Speed", justify="right")
    table.add_column("Cost (in/out)")
    table.add_column("Best For")

    for m in models:
        tier_color = {
            "pro": "red",
            "flash": "yellow",
            "lite": "green",
        }.get(m.tier.value, "white")

        table.add_row(
            m.display_name,
            f"[{tier_color}]{m.tier.value.upper()}[/{tier_color}]",
            str(m.coding),
            str(m.analysis),
            str(m.speed),
            f"${m.input_cost:.2f}/${m.output_cost:.2f}",
            ", ".join(t.value for t in m.best_for[:3]),
        )

    console.print(table)

    # Show default assignments
    router = TaskRouter()
    defaults = router.get_default_assignments()
    console.print("\n[bold]Default Task Assignments:[/bold]")
    for task_type, model_name in defaults.items():
        console.print(f"  {task_type}: [cyan]{model_name}[/cyan]")
    console.print()


@cli.command("interactive")
@click.pass_context
def interactive(ctx):
    """Interactive TUI mode for browsing and contributing."""
    from contribai.cli.tui import run_interactive

    config = load_config(ctx.obj["config_path"])
    run_interactive(config)


@cli.command("leaderboard")
@click.option(
    "--limit",
    default=20,
    help="Number of entries",
)
@click.pass_context
def show_leaderboard(ctx, limit):
    """Show contribution leaderboard and success rates."""
    from contribai.core.leaderboard import Leaderboard
    from contribai.orchestrator.memory import Memory

    async def _run():
        config = load_config(ctx.obj["config_path"])
        memory = Memory(config.storage.resolved_db_path)
        await memory.init()

        board = Leaderboard(memory._db)
        stats = await board.get_overall_stats()
        rankings = await board.get_repo_rankings(limit=limit)
        type_stats = await board.get_type_stats()

        console.print(
            Panel(
                f"Total PRs: [bold]{stats['total']}[/bold]\n"
                f"Merged: [green]{stats['merged']}[/green] | "
                f"Closed: [red]{stats['closed']}[/red] | "
                f"Open: [yellow]{stats['open']}[/yellow]\n"
                f"Merge Rate: [bold]{stats['merge_rate']}%[/bold]",
                title="Contribution Leaderboard",
            )
        )

        if rankings:
            table = Table(title="Repo Rankings")
            table.add_column("Repo", style="cyan")
            table.add_column("Total")
            table.add_column("Merged", style="green")
            table.add_column("Closed", style="red")
            table.add_column("Open", style="yellow")
            table.add_column("Rate")

            for r in rankings:
                rate_color = (
                    "green" if r.merge_rate >= 70 else "yellow" if r.merge_rate >= 40 else "red"
                )
                table.add_row(
                    r.repo,
                    str(r.total_prs),
                    str(r.merged),
                    str(r.closed),
                    str(r.open),
                    f"[{rate_color}]{r.merge_rate:.0f}%[/{rate_color}]",
                )
            console.print(table)

        if type_stats:
            table2 = Table(title="By Contribution Type")
            table2.add_column("Type", style="cyan")
            table2.add_column("Total")
            table2.add_column("Merged", style="green")
            table2.add_column("Rate")
            for t in type_stats:
                table2.add_row(
                    t.type,
                    str(t.total),
                    str(t.merged),
                    f"{t.merge_rate:.0f}%",
                )
            console.print(table2)

        await memory.close()

    asyncio.run(_run())


@cli.command("notify-test")
@click.pass_context
def notify_test(ctx):
    """Send a test notification to configured channels."""
    from contribai.notifications.notifier import (
        NotificationEvent,
        Notifier,
    )

    config = load_config(ctx.obj["config_path"])
    nc = config.notifications

    notifier = Notifier(
        slack_webhook=nc.slack_webhook,
        discord_webhook=nc.discord_webhook,
        telegram_token=nc.telegram_token,
        telegram_chat_id=nc.telegram_chat_id,
    )

    if not notifier.is_configured:
        console.print("[yellow]No notification channels configured in config.yaml[/yellow]")
        return

    async def _send():
        await notifier.notify(
            NotificationEvent(
                event_type="run_complete",
                title="Test Notification",
                message="ContribAI notifications working!",
            )
        )
        await notifier.close()

    asyncio.run(_send())
    console.print("[green]Test notification sent![/green]")


if __name__ == "__main__":
    cli()
