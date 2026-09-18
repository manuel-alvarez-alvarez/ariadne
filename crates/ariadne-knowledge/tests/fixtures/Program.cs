namespace Fixture
{
    /// <summary>Greets people.</summary>
    public class Greeter
    {
        /// <summary>Builds a greeting.</summary>
        public string Greet(string name)
        {
            return Format(name);
        }

        private string Format(string name)
        {
            return "hi " + name;
        }

        [Fact]
        public void GreetsByName()
        {
            Greet("x");
        }
    }
}
