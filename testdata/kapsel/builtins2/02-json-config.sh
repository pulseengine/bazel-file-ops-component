# package.json-style edits without jq or node: key order and indentation stay.
cat > package.json <<'EOF'
{
  "name": "demo",
  "version": "1.0.0",
  "scripts": {
    "test": "jest",
    "lint": "eslint ."
  },
  "files": ["dist", "src"]
}
EOF
printf '{"scripts": {"build": "tsc"}, "private": true}\n' > extra.json
json get -r .name package.json
json get -r .files[] package.json
json get -c .scripts package.json
json set .version '"1.1.0"' package.json
json set .scripts.test --str 'jest --ci' package.json
json del .scripts.lint package.json
json merge package.json extra.json
json get .scripts package.json
echo '{"a": {"b": [10, 20, 30]}}' | json get '.a.b[-1]'
