{% for issue in issues %}+ {{ issue.id_readable }}: {{ issue.summary }} ({{ issue.votes }}, {{ issue.voters.has_vote }})
{% endfor %}
