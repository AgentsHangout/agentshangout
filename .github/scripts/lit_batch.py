"""Collect the opt-in PR batch against main. Only this step needs a read token."""
import json
import os
from pathlib import Path
import re
import subprocess


def requested_prs(body, own_number):
    lines = [line for line in (body or "").splitlines() if line.lower().startswith("lit-with:")]
    if not lines:
        return []
    if len(lines) != 1:
        raise ValueError("Use one Lit-with: line with space-separated PR numbers.")
    numbers = lines[0].split(":", 1)[1].split()
    if not numbers or any(not re.fullmatch(r"[1-9][0-9]{0,9}", n) for n in numbers):
        raise ValueError("Lit-with: expects positive PR numbers separated by spaces.")
    selected = list(dict.fromkeys([own_number, *map(int, numbers)]))
    if len(selected) < 2:
        raise ValueError("Include at least one other PR in Lit-with:.")
    return selected


def check_sources(snapshot, event, get_pr):
    repo = event["repository"]["full_name"]
    heads = {change["number"]: change["head"] for change in snapshot["changes"]}
    expected = requested_prs(event["pull_request"]["body"], event["number"])
    if set(heads) != set(expected) or heads[event["number"]] != event["pull_request"]["head"]["sha"]:
        raise ValueError("The recorded sources do not match this pull request event.")
    for number, head in heads.items():
        pr = get_pr(number)
        if (pr["state"] != "open" or pr["draft"] or pr["base"]["ref"] != "main"
                or (pr["head"]["repo"] or {}).get("full_name") != repo or pr["head"]["sha"] != head):
            raise ValueError(f"PR #{number} must still be ready with its recorded head in this repository.")


def main():
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    repo = event["repository"]["full_name"]
    scratch = Path(os.environ["RUNNER_TEMP"]) / "lit-batch"
    bare, snapshot = scratch / "repo.git", scratch / "sources.json"
    selected = requested_prs(event["pull_request"]["body"], event["number"])
    if not selected:
        return
    scratch.mkdir()
    subprocess.run(["git", "init", "--bare", "--quiet", bare], check=True)
    subprocess.run(["lit", "collect", repo, "--branch", "main", "--repo", bare,
                    "--out", snapshot, "--prs", *map(str, selected)], check=True)

    def get_pr(number):
        return json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/pulls/{number}"]))

    check_sources(json.loads(snapshot.read_text(encoding="utf-8")), event, get_pr)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        print("pinned=true", file=output)


if __name__ == "__main__":
    main()
