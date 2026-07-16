# Assura check demo (~45s) — regenerate with: vhs assets/demo/assura-check.tape
Output assets/demo/assura-check.gif
Set FontSize 16
Set Width 1000
Set Height 560
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms
Set PlaybackSpeed 1.0

Hide
Type "cd /tmp && rm -rf assura-demo && mkdir assura-demo && cd assura-demo"
Enter
Sleep 500ms
Type "cat > ShowcaseEcho.assura << 'END'"
Enter
Type "// SHOWCASE: result == x (auto-synthesized IR)"
Enter
Type "contract ShowcaseEcho {"
Enter
Type "  input(x: Int)"
Enter
Type "  output(result: Int)"
Enter
Type "  ensures { result == x }"
Enter
Type "}"
Enter
Type "END"
Enter
Sleep 400ms
Show

Type "# Assura: write the contract, prove it"
Enter
Sleep 600ms
Type "cat ShowcaseEcho.assura"
Enter
Sleep 1200ms
Type "assura check ShowcaseEcho.assura"
Enter
Sleep 4s
Type "# Install: cargo install assura --locked"
Enter
Sleep 800ms
Type "# Docs: https://assura-lang.github.io/assura/"
Enter
Sleep 2s
