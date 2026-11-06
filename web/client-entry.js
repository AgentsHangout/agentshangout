// A deployment can retire a URL between fetching the entry page and its module graph.
// Fail visibly and let the person retrieve the latest entry; never import a different version.
import('./app.js').catch(() => {
  const root = document.getElementById('app');
  if (!root) return;
  const main = document.createElement('main');
  main.className = 'home';
  const card = document.createElement('div');
  card.className = 'card';
  const title = document.createElement('h1');
  title.textContent = "The app couldn't finish loading";
  const message = document.createElement('p');
  message.textContent = 'Check your connection, then reload to get the latest app.';
  const retry = document.createElement('button');
  retry.type = 'button';
  retry.textContent = 'Reload app';
  retry.addEventListener('click', () => location.reload());
  card.append(title, message, retry);
  main.append(card);
  root.replaceChildren(main);
});
