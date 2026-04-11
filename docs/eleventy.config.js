import syntaxHighlight from "@11ty/eleventy-plugin-syntaxhighlight";
import markdownItAnchor from "markdown-it-anchor";
import markdownItAttrs from "markdown-it-attrs";

export default function (eleventyConfig) {
  eleventyConfig.addPlugin(syntaxHighlight);

  eleventyConfig.addPassthroughCopy({ "src/assets": "assets" });

  eleventyConfig.amendLibrary("md", (mdLib) => {
    mdLib
      .use(markdownItAnchor, {
        permalink: markdownItAnchor.permalink.linkInsideHeader({
          symbol: "#",
          placement: "before",
          ariaHidden: false,
          class: "anchor",
        }),
        level: [2, 3, 4],
        slugify: (s) =>
          s
            .toLowerCase()
            .trim()
            .replace(/[^\w\s-]/g, "")
            .replace(/\s+/g, "-"),
      })
      .use(markdownItAttrs);
  });

  eleventyConfig.addFilter("docPages", (nav) =>
    nav.filter((entry) => entry.url),
  );

  eleventyConfig.addFilter("prevNext", (nav, url) => {
    const pages = nav.filter((entry) => entry.url);
    const idx = pages.findIndex((entry) => entry.url === url);
    if (idx === -1) return { prev: null, next: null };
    return {
      prev: idx > 0 ? pages[idx - 1] : null,
      next: idx < pages.length - 1 ? pages[idx + 1] : null,
    };
  });

  return {
    dir: {
      input: "src",
      output: "_site",
      includes: "_includes",
      data: "_data",
    },
    markdownTemplateEngine: "njk",
    htmlTemplateEngine: "njk",
    templateFormats: ["njk", "md", "html"],
  };
}
