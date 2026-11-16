// Countdown only. The page works without it; pitld enforces the expiry.
(function () {
  var el = document.getElementById("left");
  if (!el) return;
  var expires = parseInt(el.getAttribute("data-expires"), 10) * 1000;
  function tick() {
    var left = Math.max(0, Math.round((expires - Date.now()) / 1000));
    el.textContent = "(" + left + " s left)";
    if (left === 0) {
      el.textContent = "(expired)";
      var buttons = document.querySelectorAll("button");
      for (var i = 0; i < buttons.length; i++) buttons[i].disabled = true;
      return;
    }
    setTimeout(tick, 1000);
  }
  tick();
})();
