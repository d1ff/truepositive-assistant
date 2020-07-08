Please, review your issue:

*Summary*: {{ issue.summary|markdown_escape }}
*Project*: {{ issue.project.name }}
*Stream*: {{ issue.stream.1 }}
*Type*: {{ issue.issue_type.1 }}
*Description*:
{{ desc|markdown_escape }}

Use /save command to save the issue or /cancel to drop it.
