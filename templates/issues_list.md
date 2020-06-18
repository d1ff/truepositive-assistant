{% for issue in issues %}+ {{ issue.idReadable }}: {{ issue.summary }} ({{ issue.votes }})
{% endfor %}
