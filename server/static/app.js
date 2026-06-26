// Fade-out transition on internal navigation
document.addEventListener('DOMContentLoaded', () => {
  document.querySelectorAll('a[href^="/"]').forEach(link => {
    link.addEventListener('click', e => {
      if (link.target === '_blank') return;
      e.preventDefault();
      const href = link.getAttribute('href');
      document.body.classList.add('fade-out');
      setTimeout(() => { location.href = href; }, 150);
    });
  });
});
