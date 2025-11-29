/**
 * Database connection and query helpers
 * Simulates a database layer for Next.js server components
 */

const MOCK_DELAY = 100;

// Simulated database
const db = {
  users: new Map([
    ['1', { id: '1', name: 'Alice', email: 'alice@example.com', role: 'admin' }],
    ['2', { id: '2', name: 'Bob', email: 'bob@example.com', role: 'user' }],
  ]),
  posts: new Map([
    ['1', { id: '1', title: 'First Post', content: 'Hello world', authorId: '1', createdAt: new Date('2024-01-01') }],
    ['2', { id: '2', title: 'Second Post', content: 'More content', authorId: '2', createdAt: new Date('2024-01-15') }],
  ]),
};

async function delay(ms = MOCK_DELAY) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function findUserById(id) {
  await delay();
  return db.users.get(id) || null;
}

export async function findUserByEmail(email) {
  await delay();
  for (const user of db.users.values()) {
    if (user.email === email) {
      return user;
    }
  }
  return null;
}

export async function findAllUsers() {
  await delay();
  return Array.from(db.users.values());
}

export async function findPostById(id) {
  await delay();
  const post = db.posts.get(id);
  if (!post) return null;

  const author = await findUserById(post.authorId);
  return { ...post, author };
}

export async function findAllPosts({ limit = 10, offset = 0 } = {}) {
  await delay();
  const posts = Array.from(db.posts.values())
    .sort((a, b) => b.createdAt - a.createdAt)
    .slice(offset, offset + limit);

  return Promise.all(
    posts.map(async (post) => {
      const author = await findUserById(post.authorId);
      return { ...post, author };
    })
  );
}

export async function createPost({ title, content, authorId }) {
  await delay();
  const id = String(db.posts.size + 1);
  const post = {
    id,
    title,
    content,
    authorId,
    createdAt: new Date(),
  };
  db.posts.set(id, post);
  return post;
}
