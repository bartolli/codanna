import { notFound } from 'next/navigation';
import Link from 'next/link';
import { findPostById } from '@/lib/db';
import { formatDate } from '@/lib/utils';

export async function generateMetadata({ params }) {
  const post = await findPostById(params.id);

  if (!post) {
    return {
      title: 'Post Not Found',
    };
  }

  return {
    title: `${post.title} | NextApp`,
    description: post.content.slice(0, 160),
  };
}

export default async function PostPage({ params }) {
  const post = await findPostById(params.id);

  if (!post) {
    notFound();
  }

  return (
    <article className="max-w-3xl mx-auto">
      <nav className="mb-8">
        <Link
          href="/posts"
          className="text-blue-600 hover:text-blue-700 flex items-center"
        >
          <svg className="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
          </svg>
          Back to posts
        </Link>
      </nav>

      <header className="mb-8">
        <h1 className="text-4xl font-bold text-gray-900 mb-4">
          {post.title}
        </h1>
        <div className="flex items-center text-gray-600">
          <AuthorInfo author={post.author} />
          <span className="mx-2">|</span>
          <time dateTime={post.createdAt.toISOString()}>
            {formatDate(post.createdAt)}
          </time>
        </div>
      </header>

      <div className="prose prose-lg max-w-none">
        {post.content.split('\n').map((paragraph, i) => (
          <p key={i}>{paragraph}</p>
        ))}
      </div>
    </article>
  );
}

function AuthorInfo({ author }) {
  if (!author) {
    return <span>Unknown author</span>;
  }

  return (
    <div className="flex items-center">
      <div className="w-10 h-10 rounded-full bg-gray-200 flex items-center justify-center text-sm font-medium text-gray-600 mr-3">
        {author.name?.charAt(0).toUpperCase()}
      </div>
      <span>{author.name}</span>
    </div>
  );
}
