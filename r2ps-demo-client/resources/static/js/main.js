$(document).ready(function() {
    hljs.highlightAll();
    requestAnimationFrame(() => autoResize($('textarea.auto-grow')));
});


function autoResize($ta) {
    $ta.each(function () {
        this.style.height = 'auto';
        this.style.height = this.scrollHeight + 'px';
    });
}

