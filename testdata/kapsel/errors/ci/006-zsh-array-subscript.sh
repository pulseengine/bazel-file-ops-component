# expect: refuse KS-R000 param-operator
# source: this session: ${pipestatus[1]} is zsh-only and empty under sh
false | true
echo "exit=${pipestatus[1]}"
