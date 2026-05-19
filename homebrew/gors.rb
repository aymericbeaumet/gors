# Homebrew Formula for gors
# To be placed in https://github.com/aymericbeaumet/homebrew-tap/Formula/gors.rb
#
# This formula is automatically updated by the release workflow when a new
# version tag (e.g., v1.0.0) is pushed to the main branch.
#
# Initial setup instructions:
# 1. Create the homebrew-tap repository if it doesn't exist
# 2. Copy this file to Formula/gors.rb in that repository
# 3. Set up HOMEBREW_TAP_TOKEN secret in the gors repository
#    (a GitHub PAT with repo access to homebrew-tap)

class Gors < Formula
  desc "Experimental Go toolchain written in Rust (parser, compiler)"
  homepage "https://github.com/aymericbeaumet/gors"
  license "MIT"
  head "https://github.com/aymericbeaumet/gors.git", branch: "main"

  # These values are automatically updated by mislav/bump-homebrew-formula-action
  # when a new version tag is pushed. Do not edit manually.
  url "https://github.com/aymericbeaumet/gors/archive/refs/tags/PLACEHOLDER_TAG.tar.gz"
  sha256 "PLACEHOLDER_SHA256"
  version "PLACEHOLDER_VERSION"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "gors-cli")
  end

  test do
    # Create a simple Go file and test tokenization
    (testpath/"test.go").write <<~EOS
      package main

      func main() {
        println("Hello, World!")
      }
    EOS

    output = shell_output("#{bin}/gors tokens #{testpath}/test.go")
    assert_match "PACKAGE", output
    assert_match "FUNC", output
  end
end
