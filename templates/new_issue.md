Please, review your issue:

*Summary*: {{ issue.summary|markdown_escape }}
*Project*: {{ issue.project.name }}
*Stream*: {{ issue.stream }}
*Type*: {{ issue.issue_type }}
*Description*:
{{ desc|markdown_escape }}

Use /save command to save the issue or /cancel to drop it.
