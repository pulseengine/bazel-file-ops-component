# An edit that changes nothing fails loudly and leaves the file alone.
printf 'port = 8080\nhost = "localhost"\n' > app.conf
edit app.conf -s 'port = 9090' 'port = 9091'
echo "no match: $?"
edit app.conf -e '\d+' 'N' -c 2
echo "wrong count: $?"
edit app.conf -s 'port = 9090' 'port = 9091' --allow-none
echo "allowed: $?"
json get .missing <<'EOF'
{"present": 1}
EOF
echo "missing key: $?"
edit app.conf -e '^port = \d+$' 'port = 9090' -c 1
echo "edited: $?"
cat app.conf
