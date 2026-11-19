# Backlog

- [ ] **existing-solutions-scout — Include complete GitHub READMEs in the agent handoff.**
  The GitHub details step currently keeps only the first 700 characters of each README
  it fetches. Retrieve the full README for each GitHub result passed to the agent,
  preserve its Markdown, and attach it to the corresponding repository result.
  Use this context to assess what the project actually does, where it overlaps with
  the request, and what it could contribute. Keep the result and report when its
  README is missing or unavailable.
