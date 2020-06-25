{% for issue in issues %}{{ skip + loop.index }}. [{{ issue.id_readable }}]({{ youtrack_url }}../issue/{{ issue.id_readable }}): {{ issue.summary|markdown_escape }} ({{ issue.votes }})
{% endfor %}
