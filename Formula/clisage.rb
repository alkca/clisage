class Clisage < Formula
  desc "Shell AI assistant with encrypted credentials and Unix pipelines"
  homepage "https://github.com/alkca/clisage"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/alkca/clisage.git", branch: "main"

  depends_on "rust" => :build
  depends_on "pass"

  def fetch
    system "cargo", "fetch", *std_cargo_fetch_args
  end

  def install
    system "cargo", "install", *std_cargo_args
    generate_completions_from_executable(bin/"clisage", "completions")
  end

  def caveats
    <<~EOS
      Initialize Clisage with your existing encryption-capable GPG key:
        pass init -p clisage YOUR_GPG_KEY_ID
      Then run:
        clisage auth login

      To enable Alt-G in Zsh, add to ~/.zshrc:
        eval "$(clisage integration zsh)"

      Development installs use the HEAD formula and build main from source.
    EOS
  end

  test do
    ENV["XDG_CONFIG_HOME"] = (testpath/"config").to_s
    ENV.delete("CLISAGE_API_KEY")
    assert_match "clisage ", shell_output("#{bin}/clisage --version")
    assert_match 'credential_backend = "pass"', shell_output("#{bin}/clisage config show")
    assert_match "_clisage_zsh_widget", shell_output("#{bin}/clisage integration zsh")
    assert_match "_clisage_bash_widget", shell_output("#{bin}/clisage integration bash")
    assert_path_exists bash_completion/"clisage"
    assert_path_exists zsh_completion/"_clisage"
    assert_path_exists fish_completion/"clisage.fish"
    assert_match "provide a prompt", shell_output("#{bin}/clisage </dev/null 2>&1", 1)
  end
end
