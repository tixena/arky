---
layout: post
title: "Parsing markdown: a field guide"
date: 2026-10-03 14:30:00 -0400
categories: [engineering, parsing]
author: ada
published: true
excerpt_separator: <!--more-->
---
Markdown has no grammar. This post walks through what that means in practice.

<!--more-->

## Why it is hard

Three things get in the way:

1. Indentation decides which block a line belongs to.
2. A reference link depends on a definition elsewhere.
3. Emphasis follows rules about the characters around it.

{% highlight ruby %}
puts "liquid tags are plain text here"
{% endhighlight %}

Read the [CommonMark spec](https://spec.commonmark.org/) for the details, or the
[summary]({% post_url 2026-09-01-summary %}).
