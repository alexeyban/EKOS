using System;
using System.IO;
using System.Text;
using MarkdownSharp;

public static class MdCli
{
    public static int Main(string[] args)
    {
        var input = new StreamReader(Console.OpenStandardInput(), new UTF8Encoding(false)).ReadToEnd();
        var html = new Markdown().Transform(input);
        var stdout = Console.OpenStandardOutput();
        var bytes = new UTF8Encoding(false).GetBytes(html);
        stdout.Write(bytes, 0, bytes.Length);
        return 0;
    }
}
