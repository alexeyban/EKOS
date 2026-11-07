using System;
using System.IO;
using System.Text;
using MarkdownSharp;

// Test harness only (not part of the app being ported): documents separated by NUL in, HTML separated by NUL out.
public static class MdBatch
{
    public static int Main(string[] args)
    {
        var enc = new UTF8Encoding(false);
        var input = new StreamReader(Console.OpenStandardInput(), enc).ReadToEnd();
        var stdout = Console.OpenStandardOutput();
        var docs = input.Split('\0');
        for (int i = 0; i < docs.Length; i++)
        {
            string html;
            try { html = new Markdown().Transform(docs[i]); }
            catch (Exception e) { html = "!EXC " + e.GetType().Name; }
            var b = enc.GetBytes(html + (i < docs.Length - 1 ? "\0" : ""));
            stdout.Write(b, 0, b.Length);
        }
        return 0;
    }
}
