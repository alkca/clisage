class Clisage < Formula
  desc "Shell AI assistant with encrypted credentials and Unix pipelines"
  homepage "https://github.com/alkca/clisage"
  url "https://github.com/alkca/clisage.git",
      tag:      "v0.3.0",
      revision: "a57d727f8b6c48bcc209271098fd515c8a0f14ca"
  version "0.3.0"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/alkca/clisage.git", branch: "main"

  bottle do
    root_url "https://github.com/alkca/clisage/releases/download/v0.3.0"
    sha256 cellar: :any_skip_relocation, arm64_sequoia: "7dcf4541b9342b04e63fbb211434bf3c544a57739fd2203b8bd717eb8148a5f7"
    sha256 cellar: :any, arm64_linux:                   "7aab955a82b7e5920a24c0fabe54238a5567be93c642c968fa82fc9f14cc9fbf"
    sha256 cellar: :any, x86_64_linux:                  "6312f73be480625cdbe7faa0181189e3bb2de98fe7cd72275e152ba94b73bf76"
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
