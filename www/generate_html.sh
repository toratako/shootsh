#!/bin/bash

set -euo pipefail

cd "$(dirname "$0")"

DB_FILE="../shootsh.db"
TEMPLATE_FILE="index_template.html"
OUTPUT_FILE="index.html"

CURRENT_DATE=$(date '+%Y-%m-%d')
WORK_TMP=$(mktemp)
trap 'rm -f "$WORK_TMP"' EXIT

ROWS=$(sqlite3 "$DB_FILE" <<EOF
SELECT 
    '<tr><td>' || printf('%02d', row_number() OVER (ORDER BY s.high_score DESC, s.high_score_at ASC)) || 
    '</td><td>' || u.username || 
    '</td><td>' || s.high_score || '</td></tr>'
FROM users u
JOIN user_stats s ON u.id = s.user_id
WHERE s.high_score > 0
LIMIT 10;
EOF
)

sed -e "/LEADERBOARD_TEMPLATE/r /dev/stdin" \
    -e "s/UPDATED_TEMPLATE/${CURRENT_DATE}/g" \
    -e "/LEADERBOARD_TEMPLATE/d" \
    "$TEMPLATE_FILE" <<EOF > "$WORK_TMP"
$(echo -e "$ROWS")
EOF

chmod 644 "$WORK_TMP"
mv "$WORK_TMP" "$OUTPUT_FILE"

echo "Generated $OUTPUT_FILE"
