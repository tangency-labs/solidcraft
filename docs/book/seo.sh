#!/usr/bin/env bash
# Adds <link rel="canonical"> to every built book page and writes robots.txt and sitemap.xml.
# mdBook's templates cannot turn a chapter's .md path into its .html URL, so this runs on the built
# tree (usage: seo.sh <book-dir>). index.html and introduction.html are the same page: both point at /.
set -euo pipefail
dir="${1:?usage: seo.sh <built-book-dir>}"
base="https://docs.solidcraft.io"
cd "$dir"
urls=()
while IFS= read -r f; do
  f="${f#./}"
  case "$f" in
    404.html|toc.html) # not content pages: keep them out of the index
      sed -i 's|</head>|<meta name="robots" content="noindex"></head>|' "$f"; continue ;;
  esac
  grep -q 'rel="canonical"' "$f" && continue # mdBook's redirect stubs already name their target
  case "$f" in
    index.html|introduction.html) path="" ;;
    */index.html) path="${f%index.html}" ;;
    *) path="$f" ;;
  esac
  url="$base/$path"
  sed -i "s|</head>|<link rel=\"canonical\" href=\"$url\"></head>|" "$f"
  [ "$f" = print.html ] || [ "$f" = introduction.html ] || urls+=("$url")
done < <(find . -name '*.html' | sort)
{
  echo '<?xml version="1.0" encoding="UTF-8"?>'
  echo '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
  for u in "${urls[@]}"; do echo "  <url><loc>$u</loc></url>"; done
  echo '</urlset>'
} > sitemap.xml
printf 'User-agent: *\nAllow: /\n\nSitemap: %s/sitemap.xml\n' "$base" > robots.txt
