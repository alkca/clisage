class Clisage < Formula
  desc "Shell AI assistant with encrypted credentials and Unix pipelines"
  homepage "https://github.com/alkca/clisage"
  url "https://github.com/alkca/clisage.git",
      tag:      "v0.3.7",
      revision: "98ee375e1c9abc50c8abc1c71cf4ca7e0fb7fb57"
  version "0.3.7"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/alkca/clisage.git", branch: "main"

  bottle do
    root_url "https://github.com/alkca/clisage/releases/download/v0.3.7"
    sha256 cellar: :any_skip_relocation, arm64_sequoia: "3014ad2a024bf0986f9b1bb009846ab5ef37ebeb76ff348dd22014309cd2fa08"
    sha256 cellar: :any,                 arm64_linux:   "d35ba9c52f87891a120251930fa136fa23b06007f242f20a2bcbb0035842e206"
    sha256 cellar: :any,                 x86_64_linux:  "1947272179a11af8a4f7f93b0e2fc647c879fcdfc92b3aa22701ae27b3200b18"
  end

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
