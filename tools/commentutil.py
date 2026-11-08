import re

def blank_comments(t):
    """Replace /* ... */ blocks by spaces (keeping newlines) so line numbers stay stable."""
    return re.sub(r'/\*.*?\*/', lambda m: re.sub(r'[^\n]', ' ', m.group(0)), t, flags=re.S)
