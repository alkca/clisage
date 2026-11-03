class Clisage < Formula
  desc "Shell AI assistant with encrypted credentials and Unix pipelines"
  homepage "https://github.com/alkca/clisage"
  url "https://github.com/alkca/clisage.git",
      tag:      "v0.3.6",
      revision: "d30591e924fba504378b7345de4f0dc14e8ff19c"
  version "0.3.6"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/alkca/clisage.git", branch: "main"

  bottle do
    root_url "https://github.com/alkca/clisage/releases/download/v0.3.6"
    sha256 cellar: :any_skip_relocation, arm64_sequoia: "04ef615f4ad4023528d62ce7879fb127ec37ee7f9c21bafc78f7c36edc6ef7ef"
    sha256 cellar: :any,                 arm64_linux:   "b6a3da4c7a5b2b425ef7d850b7f50b338c49f2481ad2caebe65f427847ee9378"
    sha256 cellar: :any,                 x86_64_linux:  "32e6a031ca5e00fb6a48342eec0d804192dfd5b144044dbfc6cbbbd3b403f947"
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
