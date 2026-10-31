class Clisage < Formula
  desc "Shell AI assistant with encrypted credentials and Unix pipelines"
  homepage "https://github.com/alkca/clisage"
  url "https://github.com/alkca/clisage.git",
      tag:      "v0.3.3",
      revision: "69ffb41e70a385d2379caf488a815aa3a055302d"
  version "0.3.3"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/alkca/clisage.git", branch: "main"

  bottle do
    root_url "https://github.com/alkca/clisage/releases/download/v0.3.3"
    sha256 cellar: :any_skip_relocation, arm64_sequoia: "2324d655231ef933d4dd25550eef0ce208cdd6743d137a2e5b1ec039dfd6bbdc"
    sha256 cellar: :any,                 arm64_linux:   "b69e40e5d9bcda8fe5b9f55b5f52716c85c19d57fa7bd3340434d3873b5771fe"
    sha256 cellar: :any,                 x86_64_linux:  "b84d3632fe6fae83b91d3f4b37d24cf1c297cbea458ebc24e2df3b59e4ddc152"
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
