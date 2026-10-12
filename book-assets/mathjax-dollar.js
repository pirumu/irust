// Chapters write inline math as $...$; MathJax 2 only accepts \(...\) by default.
(function () {
  var tex2jax = {
    inlineMath: [["$", "$"], ["\\(", "\\)"]],
    displayMath: [["$$", "$$"], ["\\[", "\\]"]],
    processEscapes: true,
    // Footnotes quote sources verbatim (shell prompts, Rust macro `$x`); never typeset them.
    ignoreClass: "footnote-definition",
  };
  if (window.MathJax && window.MathJax.Hub) {
    window.MathJax.Hub.Config({ tex2jax: tex2jax });
    window.MathJax.Hub.Queue(["Typeset", window.MathJax.Hub]);
  } else {
    window.MathJax = { tex2jax: tex2jax };
  }
})();
